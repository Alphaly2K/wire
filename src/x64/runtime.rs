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
mod apc;
mod automation;
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
mod process_queries;
mod registry;
mod resources;
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
mod winhttp;
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
    tls: [Option<u64>; 64],
    fls: [Option<(u64, u64)>; 128],
    modules: crate::win32::x64::modules::Modules,
    tokens: token::Tokens,
    etw: tracing::Providers,
    function_tables: BTreeMap<u64, unwind::Table>,
    exception_handlers: Vec<(u64, u64)>,
    next_exception_handler: u64,
    profile: crate::runtime::environment::GuestEnvironment,
    environment: environment::Environment,
    local_blocks: std::collections::BTreeSet<u64>,
    task_memory: com::TaskMemory,
    bstrings: automation::Bstrings,
    hstrings: winrt::Strings,
    devices: devices::Devices,
    rng_handles: std::collections::BTreeSet<u64>,
    exe_path: String,
    pub gateways: Gateways,
    static_tls_count: u32,
    static_tls: Vec<(Vec<u8>, u64, u64)>,
    threads: threads::Threads,
    delay_hooks: BTreeMap<u64, (String, u64, u64)>,
    srw: crate::win32::kernel::srw::SrwLocks,
    exception_filter: u64,
    error_mode: u32,
    snapshots: snapshot::Snapshots,
    pub api_calls: u64,
    /// Most recent calls, bounded independently of the full api_calls counter.
    pub trace: Trace,
    events: events::Events,
    pipes: pipes::Pipes,
    files: files::Files,
    mappings: mapping::Mappings,
    winsock_refs: u32,
    security_tables: [u64; 2],
    graphics: dxgi::Graphics,
    registry: registry::Registry,
    user: user::User,
    searches: find::Searches,
    process_attributes: BTreeMap<u64, (u32, BTreeMap<u64, Vec<u8>>)>,
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
    let name = bstr_symbol(dll, dxgi::symbol(dll, sockets::symbol(dll, name)));
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
    if name == "NtQueryDirectoryFile" && !dll.eq_ignore_ascii_case("ntdll.dll") {
        return false;
    }
    if winhttp::arity(name).is_some() && !winhttp::export(dll, name) {
        return false;
    }
    if sockets::is_call(name) && !dll.eq_ignore_ascii_case("ws2_32.dll") {
        return false;
    }
    if devices::arity(name).is_some() && !devices::export(dll, name) {
        return false;
    }
    if !dll.eq_ignore_ascii_case("kernel32.dll")
        && !dll.eq_ignore_ascii_case("kernelbase.dll")
        && !(dll.eq_ignore_ascii_case("shell32.dll")
            && matches!(name, "CommandLineToArgvW" | "SHGetKnownFolderPath"))
        && !(dll.eq_ignore_ascii_case("advapi32.dll")
            && matches!(
                name,
                "OpenProcessToken"
                    | "OpenThreadToken"
                    | "GetTokenInformation"
                    | "GetSidSubAuthority"
                    | "GetSidSubAuthorityCount"
                    | "GetSidIdentifierAuthority"
                    | "IsValidSid"
                    | "GetLengthSid"
                    | "CopySid"
                    | "EqualSid"
                    | "GetSidLengthRequired"
                    | "InitializeSid"
            ))
        && !(dll.eq_ignore_ascii_case("advapi32.dll") && tracing::is_call(name))
        && !(dll.eq_ignore_ascii_case("advapi32.dll") && registry::arity(name).is_some())
        && !(dll.eq_ignore_ascii_case("ntdll.dll") && unwind::is_call(name))
        && !(dll.eq_ignore_ascii_case("ntdll.dll") && name == "NtQueryDirectoryFile")
        && !(dll.eq_ignore_ascii_case("ws2_32.dll") && sockets::is_call(name))
        && !(dll.eq_ignore_ascii_case("secur32.dll") && sspi::arity(name).is_some())
        && !(dll.eq_ignore_ascii_case("dxgi.dll") && dxgi::export(name))
        && !(dll.eq_ignore_ascii_case("d3d12.dll") && dxgi::d3d12_export(name))
        && !(dll.eq_ignore_ascii_case("d3d11.dll") && dxgi::d3d11_export(name))
        && !(dll == dxgi::DLL && dxgi::arity(name).is_some())
        && !(dll.eq_ignore_ascii_case("bcrypt.dll")
            && matches!(
                name,
                "BCryptGenRandom" | "BCryptOpenAlgorithmProvider" | "BCryptCloseAlgorithmProvider"
            ))
        && !devices::export(dll, name)
        && !winhttp::export(dll, name)
    {
        return false;
    }
    true
}
fn kernel_export(name: &str) -> bool {
    if let Some(contract) = crate::win32::catalog::by_name(name) {
        return contract
            .lookup_dlls(crate::win32::catalog::Architecture::X64)
            .contains(&"kernel32.dll");
    }
    if devices::arity(name).is_some()
        || winhttp::arity(name).is_some()
        || registry::arity(name).is_some()
        || sockets::is_call(name)
        || sspi::arity(name).is_some()
        || tracing::is_call(name)
        || dxgi::arity(name).is_some()
        || name.starts_with("BCrypt")
        || name.starts_with("time")
        || name == "NtQueryDirectoryFile"
        || matches!(
            name,
            "CommandLineToArgvW"
                | "SHGetKnownFolderPath"
                | "OpenProcessToken"
                | "OpenThreadToken"
                | "GetTokenInformation"
                | "GetSidSubAuthority"
                | "GetSidSubAuthorityCount"
                | "GetSidIdentifierAuthority"
                | "IsValidSid"
                | "GetLengthSid"
                | "CopySid"
                | "EqualSid"
                | "GetSidLengthRequired"
                | "InitializeSid"
        )
    {
        return false;
    }
    import_arity(name).is_some()
}
fn import_arity(name: &str) -> Option<usize> {
    if let Some(contract) = crate::win32::catalog::by_name(name) {
        return Some(contract.parameters.len());
    }
    Some(match name {
        name if winhttp::arity(name).is_some() => winhttp::arity(name).unwrap(),
        name if sockets::arity(name).is_some() => sockets::arity(name).unwrap(),
        name if devices::arity(name).is_some() => devices::arity(name).unwrap(),
        name if dxgi::arity(name).is_some() => dxgi::arity(name).unwrap(),
        name if registry::arity(name).is_some() => registry::arity(name).unwrap(),
        name if sspi::arity(name).is_some() => sspi::arity(name).unwrap(),
        "GetSystemDirectoryA"
        | "GetSystemDirectoryW"
        | "GetWindowsDirectoryA"
        | "GetWindowsDirectoryW"
        | "GetSystemWindowsDirectoryA"
        | "GetSystemWindowsDirectoryW" => 2,
        "AddVectoredExceptionHandler" => 2,
        "RemoveVectoredExceptionHandler" => 1,
        "RaiseException" => 4,
        "RtlUnwindEx" => 6,
        "RtlAddFunctionTable" | "RtlLookupFunctionEntry" => 3,
        "RtlDeleteFunctionTable" => 1,
        "RtlInstallFunctionTableCallback" => 6,
        "EventRegister" | "EventWrite" | "EventWriteString" | "EventSetInformation" => 4,
        "EventUnregister" => 1,
        "EventEnabled" => 2,
        "EventProviderEnabled" => 3,
        "EventWriteTransfer" => 6,
        "FormatMessageA" | "FormatMessageW" => 7,
        "GetSidSubAuthority" | "EqualSid" => 2,
        "CopySid" | "InitializeSid" => 3,
        "GetSidSubAuthorityCount"
        | "GetSidIdentifierAuthority"
        | "IsValidSid"
        | "GetLengthSid"
        | "GetSidLengthRequired" => 1,
        "OpenProcessToken" => 3,
        "OpenThreadToken" => 4,
        "GetTokenInformation" => 5,
        "SHGetKnownFolderPath" => 4,
        "CreateToolhelp32Snapshot"
        | "Process32First"
        | "Process32Next"
        | "Process32FirstW"
        | "Process32NextW"
        | "Thread32First"
        | "Thread32Next" => 2,
        "GetLogicalProcessorInformationEx" => 3,
        "BCryptGenRandom" | "BCryptOpenAlgorithmProvider" => 4,
        "BCryptCloseAlgorithmProvider" => 2,
        "NtQueryDirectoryFile" => 11,
        "CommandLineToArgvW" => 2,
        "AreFileApisANSI" => 0,
        "GetErrorMode" => 0,
        "SetErrorMode" => 1,
        "SetThreadStackGuarantee" => 1,
        "CreatePipe" => 4,
        "InitializeProcThreadAttributeList" => 4,
        "UpdateProcThreadAttribute" => 7,
        "DeleteProcThreadAttributeList" => 1,
        "CreateProcessW" => 10,
        "GetConsoleMode" | "GetTempPathW" | "GetTempPath2W" => 2,
        "EncodePointer" | "DecodePointer" | "SetUnhandledExceptionFilter" | "ExitProcess" => 1,
        "WriteConsoleW" => 5,
        _ => return None,
    })
}

fn normalize_symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    bstr_symbol(dll, dxgi::symbol(dll, sockets::symbol(dll, name)))
}
fn import_dwords(name: &str) -> &'static [usize] {
    if let Some(contract) = crate::win32::catalog::by_name(name) {
        return contract.win64_dwords();
    }
    match name {
        name if winhttp::arity(name).is_some() => winhttp::dwords(name),
        name if sockets::arity(name).is_some() => sockets::dwords(name),
        name if devices::arity(name).is_some() => devices::dwords(name),
        name if dxgi::arity(name).is_some() => dxgi::dwords(name),
        name if registry::arity(name).is_some() => registry::dwords(name),
        name if sspi::arity(name).is_some() => sspi::dwords(name),
        "GetSystemDirectoryA"
        | "GetSystemDirectoryW"
        | "GetWindowsDirectoryA"
        | "GetWindowsDirectoryW"
        | "GetSystemWindowsDirectoryA"
        | "GetSystemWindowsDirectoryW" => &[1],
        "WSASetLastError" => &[0],
        "AddVectoredExceptionHandler" => &[0],
        "RaiseException" => &[0, 1, 2],
        "RtlAddFunctionTable" => &[1],
        "RtlInstallFunctionTableCallback" => &[2],
        "EventWrite" => &[2],
        "EventWriteTransfer" => &[4],
        "EventSetInformation" => &[1, 3],
        "FormatMessageA" | "FormatMessageW" => &[0, 2, 3, 5],
        "GetSidSubAuthority" => &[1],
        "CopySid" => &[0],
        "OpenProcessToken" => &[1],
        "OpenThreadToken" => &[1, 2],
        "GetTokenInformation" => &[1, 3],
        "SHGetKnownFolderPath" => &[1],
        "CreateToolhelp32Snapshot" => &[0, 1],
        "GetLogicalProcessorInformationEx" => &[0],
        "BCryptGenRandom" => &[2, 3],
        "BCryptOpenAlgorithmProvider" => &[3],
        "BCryptCloseAlgorithmProvider" => &[1],
        "NtQueryDirectoryFile" => &[6, 7],
        "SetErrorMode" => &[0],
        "InitializeProcThreadAttributeList" => &[1, 2],
        "UpdateProcThreadAttribute" => &[1],
        "CreateProcessW" => &[4, 5],
        "CreatePipe" => &[3],
        "GetTempPathW" | "GetTempPath2W" | "ExitProcess" => &[0],
        "WriteConsoleW" => &[2],
        _ => &[],
    }
}
fn import_groups(name: &str) -> u32 {
    let predicates = [
        false, // BSTR moved to catalog typed IDs; preserve legacy bit positions
        winhttp::arity(name).is_some(),
        devices::arity(name).is_some(),
        false, // message queues moved to catalog control IDs; preserve bit positions
        false, // identity moved to catalog typed IDs; preserve legacy bit positions
        false, // WinRT moved to catalog typed IDs; preserve legacy bit positions
        false, // GUID helpers moved to catalog typed IDs; preserve legacy bit positions
        dxgi::arity(name).is_some(),
        false, // bound directory-search contracts supply bit 8; preserve bit positions
        false, // WNDPROC controls moved to catalog; preserve bit positions
        false, // scalar window controls moved to catalog; preserve bit positions
        registry::arity(name).is_some(),
        false, // display moved to catalog typed/control IDs; preserve bit positions
        sspi::arity(name).is_some(),
        sockets::is_call(name),
        false, // timers moved to catalog typed IDs; preserve legacy bit positions
    ];
    predicates
        .into_iter()
        .enumerate()
        .fold(0, |mask, (n, yes)| mask | ((yes as u32) << n))
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
            tls: [None; 64],
            fls: [None; 128],
            modules: crate::win32::x64::modules::Modules::new(TEB + 0x20000),
            tokens: token::Tokens::default(),
            etw: tracing::Providers::default(),
            function_tables: BTreeMap::new(),
            exception_handlers: Vec::new(),
            next_exception_handler: 0x7fff8000,
            profile: crate::runtime::environment::GuestEnvironment::amd64(),
            environment: Default::default(),
            local_blocks: std::collections::BTreeSet::new(),
            task_memory: Default::default(),
            hstrings: Default::default(),
            devices: Default::default(),
            rng_handles: std::collections::BTreeSet::new(),
            exe_path: command.trim_matches('"').to_owned(),
            gateways: BTreeMap::new().into(),
            static_tls_count: 0,
            static_tls: Vec::new(),
            threads: threads::Threads::new(stack_size),
            delay_hooks: BTreeMap::new(),
            srw: Default::default(),
            exception_filter: 0,
            error_mode: 0,
            snapshots: snapshot::Snapshots::default(),
            api_calls: 0,
            trace: Trace::default(),
            events: events::Events::default(),
            pipes: pipes::Pipes::default(),
            files: files::Files::default(),
            mappings: mapping::Mappings::default(),
            winsock_refs: 0,
            security_tables: [0; 2],
            graphics: dxgi::Graphics::default(),
            registry: registry::Registry::default(),
            user: user::User::default(),
            searches: find::Searches::default(),
            process_attributes: BTreeMap::new(),
        })
    }
    fn set_error(&self, mem: &mut Memory, error: u32) -> Result<(), String> {
        mem.write_int(self.teb() + 0x68, 4, error as u64)
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
        let mut bytes = Vec::new();
        for n in 0..32768u64 {
            let u = mem.read_int(ptr.checked_add(n).ok_or("string overflow")?, 1)? as u8;
            if u == 0 {
                return String::from_utf8(bytes)
                    .map_err(|_| "non-ASCII Win64 export/module name".into());
            }
            bytes.push(u);
        }
        Err("unterminated Win64 export/module name".into())
    }
    fn path_name(mem: &Memory, pointer: u64, wide: bool) -> Result<String, String> {
        if wide {
            Self::utf16(mem, pointer)
        } else {
            let bytes = Self::string_bytes(mem, pointer, u32::MAX as u64, 1)?;
            crate::codepage::decode(&bytes[..bytes.len() - 1], 932)
                .map_err(|e| format!("path codepage error {e}"))
        }
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
        let name = bstr_symbol(dll, dxgi::symbol(dll, sockets::symbol(dll, name)));
        let export = format!("{dll}!{name}");
        if let Some((&address, _)) = self.gateways.iter().find(|(_, value)| **value == export) {
            return address;
        }
        let address = 0x7ffe00000000 + self.gateways.len() as u64 * 16;
        self.gateways.insert(address, export);
        address
    }
    fn heap_alloc(&mut self, mem: &mut Memory, size: u64) -> Result<u64, String> {
        if size > 16 * 1024 * 1024 {
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        let span = size.max(1).div_ceil(PAGE) * PAGE;
        let address = self.next_alloc;
        if let Err(error) = mem.map(address, span, READ | WRITE) {
            self.trace_note(format!(
                "Win64 allocation failed: address={address:#x} size={size} span={span}: {error}"
            ));
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        self.next_alloc += span;
        self.allocations.insert(address, (size as usize, span));
        Ok(address)
    }
    fn string_bytes(mem: &Memory, ptr: u64, count: u64, width: usize) -> Result<Vec<u8>, String> {
        crate::win32::x64::text::string_bytes(mem, ptr, count, width)
    }
    fn processor_information(relation: u32) -> Option<Vec<u8>> {
        if !matches!(relation, 0 | 1 | 2 | 3 | 4 | 6 | 0xffff) {
            return None;
        }
        let mut output = Vec::new();
        // The interpreter exposes one virtual CPU, one NUMA node and one group.
        // No hardware cache is advertised (also consistent with the CPUID profile).
        for (kind, size) in [(0u32, 48usize), (1, 48), (3, 48), (4, 80), (6, 48)] {
            if kind != relation && !(relation == 0xffff && kind != 6) {
                continue;
            }
            let mut record = vec![0; size];
            record[..4].copy_from_slice(&kind.to_le_bytes());
            record[4..8].copy_from_slice(&(size as u32).to_le_bytes());
            if kind == 4 {
                record[8..10].copy_from_slice(&1u16.to_le_bytes());
                record[10..12].copy_from_slice(&1u16.to_le_bytes());
                record[32] = 1;
                record[33] = 1;
                record[72..80].copy_from_slice(&1u64.to_le_bytes());
            } else {
                if kind != 1 {
                    record[30..32].copy_from_slice(&1u16.to_le_bytes());
                }
                record[32..40].copy_from_slice(&1u64.to_le_bytes());
            }
            output.extend(record);
        }
        Some(output)
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
            ApiId::Com(id) => self.com_call(mem, id, args),
            ApiId::Winrt(id) => self.winrt_call(mem, id, args),
            ApiId::Bstr(id) => self.bstr_call(mem, id, args),
            ApiId::Variant(_) => unreachable!("Variant helpers have no Win64 binding"),
            ApiId::ProcessQuery(id) => self.process_query_call(mem, id, args),
            ApiId::Environment(id) => self.environment_call(mem, id, args),
            ApiId::Startup(id) => {
                crate::win32::x64::process_startup::call(id, args, mem, [COMMAND_A, COMMAND_W])
            }
            ApiId::Keyboard(id) => self.keyboard_call(mem, id, args),
            ApiId::Cursor(id) => self.cursor_call(mem, id, args),
            ApiId::Image(id) => self.image_call(mem, id, args),
            ApiId::Display(id) => self.display_call(mem, id, args),
            ApiId::GdiDc(id) => self.gdi_dc_call(mem, id, args),
            ApiId::Desktop(id) => self.desktop_call(mem, id, args),
            ApiId::Class(id) => self.class_call(mem, id, args),
            ApiId::WindowQuery(id) => self.window_query_call(mem, id, args),
            ApiId::WindowState(id) => self.window_state_call(mem, id, args),
            ApiId::WindowControl(id) => self.window_value_call(mem, id, args),
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
            ApiId::BasicString(_) | ApiId::PointerHelper(_) => {
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
            ApiId::Heap(id) => self.guest_heap_call(mem, id, args),
            ApiId::LocalMemory(id) => self.local_memory_call(mem, id, args),
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
            _ if binding.groups & (1 << 1) != 0 => self.winhttp_call(mem, name, args)?,
            _ if binding.groups & (1 << 2) != 0 => self.device_call(mem, name, args)?,
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
            _ if binding.groups & (1 << 7) != 0 => self.dxgi_call(mem, name, args)?,
            _ if binding.groups & (1 << 8) != 0 => self.find_call(mem, file.unwrap(), args)?,
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
            _ if binding.groups & (1 << 11) != 0 => self.registry_call(mem, name, args)?,
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
            _ if binding.groups & (1 << 13) != 0 => self.security_call(mem, name, args)?,
            "GetSystemDirectoryA"
            | "GetSystemDirectoryW"
            | "GetWindowsDirectoryA"
            | "GetWindowsDirectoryW"
            | "GetSystemWindowsDirectoryA"
            | "GetSystemWindowsDirectoryW" => {
                let path = if name.starts_with("GetSystemDirectory") {
                    "C:\\Windows\\System32\0"
                } else {
                    "C:\\Windows\0"
                };
                let wide = name.ends_with('W');
                let bytes = if wide {
                    path.encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    path.as_bytes().to_vec()
                };
                let required = bytes.len() / if wide { 2 } else { 1 };
                if args[1] < required as u64 {
                    required as u64
                } else {
                    mem.write(args[0], &bytes)?;
                    required as u64 - 1
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
            _ if binding.groups & (1 << 14) != 0 && name != "WSAWaitForMultipleEvents" => {
                self.winsock_call(mem, name, args)?
            }
            "AddVectoredExceptionHandler" => {
                if args[1] == 0 {
                    self.set_error(mem, 87)?;
                    0
                } else if self.exception_handlers.len() >= 64
                    || self.next_exception_handler >= 0x7fff8100
                {
                    self.set_error(mem, 8)?;
                    0
                } else {
                    let handle = self.next_exception_handler;
                    self.next_exception_handler += 1;
                    if args[0] != 0 {
                        self.exception_handlers.insert(0, (handle, args[1]));
                    } else {
                        self.exception_handlers.push((handle, args[1]));
                    }
                    handle
                }
            }
            "RemoveVectoredExceptionHandler" => {
                if let Some(index) = self
                    .exception_handlers
                    .iter()
                    .position(|&(handle, _)| handle == args[0])
                {
                    self.exception_handlers.remove(index);
                    1
                } else {
                    0
                }
            }
            "RaiseException" => {
                if args[2] > 15 {
                    return Err("Win64 RaiseException parameter count exceeds 15".into());
                }
                if args[2] != 0 {
                    mem.check(args[3], args[2] as usize * 8, READ)?;
                }
                if args[1] & !1 != 0 || !self.threads.enabled {
                    return Err("unsupported Win64 software exception flags/scheduler".into());
                }
                if self.exception_handlers.is_empty()
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
            "RtlUnwindEx" => {
                return Ok(Control::Unwind {
                    dll: dll.into(),
                    args: args.to_vec(),
                })
            }
            name if unwind::is_call(name) => match self.function_table_call(mem, name, args)? {
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
            },
            name if tracing::is_call(name) => self.etw.call(mem, name, args)?,
            "FormatMessageA" | "FormatMessageW" => self.format_message(mem, name, args)?,
            "GetSidSubAuthority"
            | "GetSidSubAuthorityCount"
            | "GetSidIdentifierAuthority"
            | "IsValidSid"
            | "GetLengthSid"
            | "GetSidLengthRequired"
            | "InitializeSid"
            | "CopySid"
            | "EqualSid" => {
                if name == "InitializeSid" && args[2] as u8 <= 15 {
                    mem.check(args[0], 8 + args[2] as u8 as usize * 4, WRITE)?;
                    mem.check(args[1], 6, READ)?;
                }
                if name == "CopySid" {
                    if let Ok(sid) = token::sid(mem, args[2]) {
                        if args[0] >= sid.len() as u64 {
                            mem.check(args[1], sid.len(), WRITE)?;
                        }
                    }
                }
                let result = (|| -> Result<u64, u32> {
                    if name == "GetSidLengthRequired" {
                        return Ok(8 + (args[0] as u8 as u64) * 4);
                    }
                    if name == "InitializeSid" {
                        let count = args[2] as u8;
                        if count > 15 {
                            return Err(87);
                        }
                        let mut header = [1, count, 0, 0, 0, 0, 0, 0];
                        mem.read(args[1], &mut header[2..]).map_err(|_| 1337u32)?;
                        mem.check(args[0], 8 + count as usize * 4, WRITE)
                            .map_err(|_| 1337u32)?;
                        mem.write(args[0], &header).map_err(|_| 1337u32)?;
                        return Ok(1);
                    }
                    let source = if name == "CopySid" { args[2] } else { args[0] };
                    let sid = token::sid(mem, source)?;
                    Ok(match name {
                        "IsValidSid" => 1,
                        "GetLengthSid" => sid.len() as u64,
                        "GetSidIdentifierAuthority" => source + 2,
                        "GetSidSubAuthorityCount" => source + 1,
                        "GetSidSubAuthority" => {
                            if args[1] >= sid[1] as u64 {
                                return Err(87);
                            }
                            source + 8 + args[1] * 4
                        }
                        "EqualSid" => (sid == token::sid(mem, args[1])?) as u64,
                        "CopySid" => {
                            if args[0] < sid.len() as u64 {
                                return Err(122);
                            }
                            mem.write(args[1], &sid).map_err(|_| 1337u32)?;
                            1
                        }
                        _ => unreachable!(),
                    })
                })();
                match result {
                    Ok(value) => value,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "OpenProcessToken" => {
                let last_error = self.teb() + 0x68;
                self.tokens.open(mem, args, last_error)?
            }
            "OpenThreadToken" => {
                self.set_error(
                    mem,
                    if self.threads.id(args[0]).is_ok() {
                        1008
                    } else {
                        6
                    },
                )?;
                0
            }
            "GetTokenInformation" => {
                let last_error = self.teb() + 0x68;
                self.tokens.query(mem, args, last_error)?
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
                let id = file.unwrap();
                let path = Self::path_name(mem, args[0], id.wide())?;
                let moving = matches!(
                    id,
                    FileApi::MoveFileA
                        | FileApi::MoveFileW
                        | FileApi::MoveFileExA
                        | FileApi::MoveFileExW
                );
                let destination = if moving && args[1] != 0 {
                    Some(Self::path_name(mem, args[1], id.wide())?)
                } else {
                    None
                };
                let mutation = if moving {
                    files::PathMutation::Move {
                        destination: destination.as_deref(),
                        flags: if matches!(id, FileApi::MoveFileExA | FileApi::MoveFileExW) {
                            args[2]
                        } else {
                            0
                        },
                    }
                } else if matches!(id, FileApi::RemoveDirectoryA | FileApi::RemoveDirectoryW) {
                    files::PathMutation::RemoveDirectory
                } else {
                    files::PathMutation::DeleteFile
                };
                let result = self.files.mutate_path(&path, mutation);
                match result {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
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
                let path = Self::path_name(mem, args[0], file.unwrap().wide())?;
                let result = self.files.set_attributes(&path, args[1]);
                match result {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "SHGetKnownFolderPath" => {
                mem.check(args[3], 8, WRITE)?;
                let mut guid = [0; 16];
                mem.read(args[0], &mut guid)?;
                let path = match guid {
                    [0xa4, 0xa1, 0x20, 0xa5, 0x80, 0x17, 0xf6, 0x4f, 0xbd, 0x18, 0x16, 0x73, 0x43, 0xc5, 0xaf, 0x16] => {
                        Some(self.profile.local_low())
                    }
                    [0x85, 0x27, 0xb3, 0xf1, 0xba, 0x6f, 0xcf, 0x4f, 0x9d, 0x55, 0x7b, 0x8e, 0x7f, 0x15, 0x70, 0x91] => {
                        Some(self.profile.local())
                    }
                    [0xdb, 0x85, 0xb6, 0x3e, 0xf9, 0x65, 0xf6, 0x4c, 0xa0, 0x3a, 0xe3, 0xef, 0x65, 0x72, 0x9f, 0x3d] => {
                        Some(self.profile.roaming())
                    }
                    [0x8f, 0x85, 0x6c, 0x5e, 0x22, 0x0e, 0x60, 0x47, 0x9a, 0xfe, 0xea, 0x33, 0x17, 0xb6, 0x71, 0x73] => {
                        Some(self.profile.home())
                    }
                    [0xd0, 0x9a, 0xd3, 0xfd, 0x8f, 0x23, 0xaf, 0x46, 0xad, 0xb4, 0x6c, 0x85, 0x48, 0x03, 0x69, 0xc7] => {
                        Some(self.profile.documents())
                    }
                    _ => None,
                };
                let data = (|| -> Result<Vec<u8>, u32> {
                    if args[2] != 0 || args[1] & !0xc000 != 0 {
                        return Err(50);
                    }
                    let path = path.ok_or(50u32)?;
                    let host = self.files.filesystem().ok_or(3u32)?.resolve(path)?;
                    if args[1] & 0x8000 != 0 {
                        std::fs::create_dir_all(&host).map_err(|e| crate::fs::win_error(&e))?;
                    }
                    if args[1] & 0x4000 == 0 && !host.is_dir() {
                        return Err(3);
                    }
                    Ok(path
                        .encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect())
                })();
                match data {
                    Err(error) => {
                        mem.write_int(args[3], 8, 0)?;
                        0x80070000 | error as u64
                    }
                    Ok(data) => {
                        let p = self.heap_alloc(mem, data.len() as u64)?;
                        if p == 0 {
                            mem.write_int(args[3], 8, 0)?;
                            0x8007000e
                        } else {
                            mem.write(p, &data)?;
                            mem.write_int(args[3], 8, p)?;
                            self.task_memory.track(p);
                            0
                        }
                    }
                }
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
                let path = Self::path_name(mem, args[0], file.unwrap().wide())?;
                let result = self
                    .files
                    .filesystem()
                    .ok_or("Win64 directory requires filesystem")?
                    .resolve(&path)
                    .and_then(|p| {
                        std::fs::create_dir(p).map_err(|e| {
                            if e.kind() == std::io::ErrorKind::AlreadyExists {
                                183
                            } else {
                                crate::fs::win_error(&e)
                            }
                        })
                    });
                match result {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
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
            "CreateToolhelp32Snapshot" => {
                let threads = self
                    .threads
                    .entries
                    .iter()
                    .filter(|(_, t)| t.exit.is_none())
                    .map(|(&id, t)| (id as u32, t.priority));
                match self.snapshots.create(args[0], &self.exe_path, threads) {
                    Ok(handle) => handle,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        u64::MAX
                    }
                }
            }
            "Process32First" | "Process32Next" | "Process32FirstW" | "Process32NextW"
            | "Thread32First" | "Thread32Next" => {
                let last_error = self.teb() + 0x68;
                self.snapshots.enumerate(mem, name, args, last_error)?
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
            "GetLogicalProcessorInformationEx" => {
                if let Some(data) = Self::processor_information(args[0] as u32) {
                    mem.check(args[2], 4, WRITE)?;
                    let capacity = mem.read_int(args[2], 4)? as usize;
                    if capacity < data.len() {
                        mem.write_int(args[2], 4, data.len() as u64)?;
                        self.set_error(mem, 122)?;
                        0
                    } else {
                        mem.check(args[1], data.len(), WRITE)?;
                        mem.write(args[1], &data)?;
                        mem.write_int(args[2], 4, data.len() as u64)?;
                        1
                    }
                } else {
                    self.set_error(mem, 87)?;
                    0
                }
            }
            "BCryptGenRandom" => {
                if args[3] & !3 != 0 || args[3] & 2 != 0 && args[0] != 0 {
                    0xc000000d
                } else if args[3] & 2 == 0
                    && args[0] != 0x81
                    && !self.rng_handles.contains(&args[0])
                {
                    0xc0000008
                } else if args[2] > 16 * 1024 * 1024 {
                    0xc0000017
                } else {
                    mem.check(args[1], args[2] as usize, WRITE)?;
                    let mut data = vec![0; args[2] as usize];
                    let mut success = true;
                    for bytes in data.chunks_mut(256) {
                        if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0
                        {
                            success = false;
                            break;
                        }
                    }
                    if success {
                        mem.write(args[1], &data)?;
                        0
                    } else {
                        0xc0000001
                    }
                }
            }
            "BCryptOpenAlgorithmProvider" => {
                let algorithm = Self::utf16(mem, args[1])?;
                let implementation = if args[2] == 0 {
                    None
                } else {
                    Some(Self::utf16(mem, args[2])?)
                };
                if args[3] != 0 {
                    0xc000000d
                } else if algorithm != "RNG"
                    || implementation
                        .as_ref()
                        .is_some_and(|name| name != "Microsoft Primitive Provider")
                {
                    0xc00000bb
                } else {
                    mem.check(args[0], 8, WRITE)?;
                    let handle = (0..1024u64)
                        .map(|n| TEB + 0x700000 + n * 16)
                        .find(|h| !self.rng_handles.contains(h));
                    if let Some(handle) = handle {
                        mem.write_int(args[0], 8, handle)?;
                        self.rng_handles.insert(handle);
                        0
                    } else {
                        0xc0000017
                    }
                }
            }
            "BCryptCloseAlgorithmProvider" => {
                if args[1] != 0 {
                    0xc000000d
                } else if self.rng_handles.remove(&args[0]) {
                    0
                } else {
                    0xc0000008
                }
            }
            _ if matches!(
                file,
                Some(FileApi::CreateFileMappingW | FileApi::CreateFileMappingA)
            ) =>
            {
                let inherit = if args[1] == 0 {
                    false
                } else {
                    mem.check(args[1], 24, READ)?;
                    if mem.read_int(args[1], 4)? != 24 || mem.read_int(args[1] + 8, 8)? != 0 {
                        return Err("Win64 mapping security descriptor not implemented".into());
                    }
                    mem.read_int(args[1] + 16, 4)? != 0
                };
                let result = if args[5] != 0 || args[2] != 2 || args[0] == u64::MAX {
                    Err(50)
                } else {
                    self.files.mapping_file(args[0]).and_then(|file| {
                        self.mappings.create(file, args[3] << 32 | args[4], inherit)
                    })
                };
                match result {
                    Ok(handle) => handle,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            _ if matches!(
                file,
                Some(FileApi::MapViewOfFile | FileApi::MapViewOfFileEx)
            ) =>
            {
                match self.mappings.map(
                    mem,
                    args[0],
                    args[1] as u32,
                    args[2] << 32 | args[3],
                    args[4],
                    if file == Some(FileApi::MapViewOfFileEx) {
                        args[5]
                    } else {
                        0
                    },
                ) {
                    Ok(base) => base,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            _ if file == Some(FileApi::UnmapViewOfFile) => {
                if self.mappings.contains_view(args[0]) {
                    self.mappings.unmap(mem, args[0])?;
                    1
                } else {
                    self.set_error(mem, 487)?;
                    0
                }
            }
            _ if file == Some(FileApi::FlushViewOfFile) => {
                match self.mappings.flush(args[0], args[1]) {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            _ if matches!(file, Some(FileApi::CreateFileW | FileApi::CreateFileA)) => {
                if args[6] != 0 {
                    return Err("Win64 file template handle not implemented".into());
                }
                let path = Self::path_name(mem, args[0], file.unwrap().wide())?;
                let inherit = if args[3] == 0 {
                    false
                } else {
                    mem.check(args[3], 24, READ)?;
                    if mem.read_int(args[3], 4)? != 24 || mem.read_int(args[3] + 8, 8)? != 0 {
                        return Err("Win64 file security descriptor not implemented".into());
                    }
                    mem.read_int(args[3] + 16, 4)? != 0
                };
                let result = self
                    .files
                    .filesystem()
                    .ok_or("Win64 file open has no mounted filesystem")?
                    .resolve(&path)
                    .and_then(|path| {
                        if args[4] == 2 || args[4] == 5 {
                            if let Ok(meta) = std::fs::metadata(&path) {
                                if self.mappings.has_file(&meta)? {
                                    return Err(1224);
                                }
                            }
                        }
                        self.files.open(
                            path,
                            args[1] as u32,
                            args[2] as u32,
                            args[4] as u32,
                            args[5] as u32,
                            inherit,
                        )
                    });
                match result {
                    Ok((handle, existed)) => {
                        if args[4] == 2 || args[4] == 4 {
                            self.set_error(mem, if existed { 183 } else { 0 })?;
                        }
                        handle
                    }
                    Err(error) => {
                        self.set_error(mem, error)?;
                        u64::MAX
                    }
                }
            }
            "NtQueryDirectoryFile" => self.nt_query_directory(mem, args)?,
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
                )
            ) =>
            {
                let result = (|| -> Result<Vec<u8>, u32> {
                    let (meta, path) = self.files.metadata(args[0])?;
                    let attributes = files::attributes(
                        &meta,
                        path.canonicalize()
                            .ok()
                            .is_some_and(|p| self.files.is_not_indexed(&p)),
                    );
                    let kind = if file == Some(FileApi::GetFileInformationByHandleEx) {
                        args[1]
                    } else {
                        u64::MAX
                    };
                    let mut data = match kind {
                        u64::MAX if file == Some(FileApi::GetFileTime) => vec![0; 24],
                        u64::MAX => vec![0; 52],
                        0 => vec![0; 40],  // FILE_BASIC_INFO
                        1 => vec![0; 24],  // FILE_STANDARD_INFO
                        9 => vec![0; 8], // FILE_ATTRIBUTE_TAG_INFO: host regular files have no reparse tag.
                        18 => vec![0; 24], // FILE_ID_INFO: real host volume and inode identity.
                        _ => return Err(50),
                    };
                    if file == Some(FileApi::GetFileInformationByHandleEx)
                        && args[3] < data.len() as u64
                    {
                        return Err(24);
                    }
                    if kind == u64::MAX || kind == 0 {
                        let offset = if file == Some(FileApi::GetFileInformationByHandle) {
                            4
                        } else {
                            0
                        };
                        for (n, time) in [meta.created(), meta.accessed(), meta.modified()]
                            .into_iter()
                            .enumerate()
                        {
                            // GetFileTime permits independent null outputs.
                            if file != Some(FileApi::GetFileTime) || args[n + 1] != 0 {
                                data[offset + n * 8..offset + n * 8 + 8]
                                    .copy_from_slice(&files::filetime(time)?.to_le_bytes());
                            }
                        }
                    }
                    match kind {
                        u64::MAX if file == Some(FileApi::GetFileTime) => {}
                        u64::MAX => {
                            let (volume, index, links, _) = files::identity(&meta)?;
                            data[..4].copy_from_slice(&attributes.to_le_bytes());
                            data[28..32].copy_from_slice(&(volume as u32).to_le_bytes());
                            data[32..36]
                                .copy_from_slice(&((meta.len() >> 32) as u32).to_le_bytes());
                            data[36..40].copy_from_slice(&(meta.len() as u32).to_le_bytes());
                            data[40..44].copy_from_slice(&links.to_le_bytes());
                            data[44..48].copy_from_slice(&((index >> 32) as u32).to_le_bytes());
                            data[48..52].copy_from_slice(&(index as u32).to_le_bytes());
                        }
                        0 => {
                            data[24..32].copy_from_slice(&files::change_time(&meta)?.to_le_bytes());
                            data[32..36].copy_from_slice(&attributes.to_le_bytes());
                        }
                        1 => {
                            let (_, _, links, allocated) = files::identity(&meta)?;
                            data[..8].copy_from_slice(&allocated.to_le_bytes());
                            data[8..16].copy_from_slice(&meta.len().to_le_bytes());
                            data[16..20].copy_from_slice(&links.to_le_bytes());
                            // FILE_FLAG_DELETE_ON_CLOSE is rejected at open, so pending is false.
                            data[21] = meta.is_dir() as u8;
                        }
                        9 => data[..4].copy_from_slice(&attributes.to_le_bytes()),
                        18 => {
                            let (volume, index, _, _) = files::identity(&meta)?;
                            data[..8].copy_from_slice(&volume.to_le_bytes());
                            data[8..16].copy_from_slice(&index.to_le_bytes());
                        }
                        _ => unreachable!(),
                    }
                    Ok(data)
                })();
                match result {
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                    Ok(data) if file == Some(FileApi::GetFileTime) => {
                        for &address in &args[1..4] {
                            if address != 0 {
                                mem.check(address, 8, WRITE)?;
                            }
                        }
                        for (n, &address) in args[1..4].iter().enumerate() {
                            if address != 0 {
                                mem.write(address, &data[n * 8..n * 8 + 8])?;
                            }
                        }
                        1
                    }
                    Ok(data) => {
                        mem.write(
                            args[if file == Some(FileApi::GetFileInformationByHandle) {
                                1
                            } else {
                                2
                            }],
                            &data,
                        )?;
                        1
                    }
                }
            }
            _ if matches!(
                file,
                Some(FileApi::GetFileSizeEx | FileApi::GetFileSize | FileApi::FlushFileBuffers)
            ) =>
            {
                let result = if file == Some(FileApi::FlushFileBuffers) {
                    self.files.flush(args[0])
                } else {
                    self.files.size(args[0])
                };
                match result {
                    Ok(value) if file == Some(FileApi::GetFileSizeEx) => {
                        mem.write_int(args[1], 8, value)?;
                        1
                    }
                    Ok(value) if file == Some(FileApi::GetFileSize) => {
                        if args[1] != 0 {
                            mem.write_int(args[1], 4, value >> 32)?;
                        }
                        self.set_error(mem, 0)?;
                        value as u32 as u64
                    }
                    Ok(value) => value,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        if file == Some(FileApi::GetFileSize) {
                            u32::MAX as u64
                        } else {
                            0
                        }
                    }
                }
            }
            _ if file == Some(FileApi::SetFilePointerEx) => {
                if args[2] != 0 {
                    mem.check(args[2], 8, WRITE)?;
                }
                match self.files.seek(args[0], args[1] as i64, args[3] as u32) {
                    Ok(position) => {
                        if args[2] != 0 {
                            mem.write_int(args[2], 8, position)?;
                        }
                        1
                    }
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            _ if matches!(
                file,
                Some(
                    FileApi::GetFileAttributesW
                        | FileApi::GetFileAttributesA
                        | FileApi::GetFileAttributesExW
                        | FileApi::GetFileAttributesExA
                )
            ) =>
            {
                let extended = matches!(
                    file,
                    Some(FileApi::GetFileAttributesExW | FileApi::GetFileAttributesExA)
                );
                if extended && args[1] != 0 {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let wide = file.unwrap().wide();
                    let path = Self::path_name(mem, args[0], wide)?;
                    let fs = self
                        .files
                        .filesystem()
                        .ok_or("Win64 file query has no mounted filesystem")?;
                    match fs.resolve(&path).and_then(|p| {
                        std::fs::metadata(&p)
                            .map(|meta| (p, meta))
                            .map_err(|e| crate::fs::win_error(&e))
                    }) {
                        Ok((path, meta)) => {
                            let attributes = (if meta.is_dir() { 0x10 } else { 0 })
                                | if path
                                    .canonicalize()
                                    .ok()
                                    .is_some_and(|p| self.files.is_not_indexed(&p))
                                {
                                    0x2000
                                } else {
                                    0
                                }
                                | meta.permissions().readonly() as u64;
                            let attributes = if attributes == 0 { 0x80 } else { attributes };
                            if extended {
                                let mut data = [0; 36];
                                data[..4].copy_from_slice(&(attributes as u32).to_le_bytes());
                                for (offset, time) in [
                                    (4, meta.created()),
                                    (12, meta.accessed()),
                                    (20, meta.modified()),
                                ] {
                                    let time = time.map_err(|e| {
                                        format!("host file timestamp unavailable: {e}")
                                    })?;
                                    let ticks = match time.duration_since(UNIX_EPOCH) {
                                        Ok(d) => {
                                            116444736000000000i128 + (d.as_nanos() / 100) as i128
                                        }
                                        Err(e) => {
                                            116444736000000000i128
                                                - (e.duration().as_nanos() / 100) as i128
                                        }
                                    };
                                    let ticks = u64::try_from(ticks)
                                        .map_err(|_| "file timestamp outside FILETIME range")?;
                                    data[offset..offset + 8].copy_from_slice(&ticks.to_le_bytes());
                                }
                                let size = if meta.is_dir() { 0 } else { meta.len() };
                                data[28..32].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
                                data[32..36].copy_from_slice(&(size as u32).to_le_bytes());
                                mem.write(args[2], &data)?;
                                1
                            } else {
                                attributes
                            }
                        }
                        Err(error) => {
                            self.set_error(mem, error)?;
                            if extended {
                                0
                            } else {
                                u32::MAX as u64
                            }
                        }
                    }
                }
            }
            _ if matches!(
                file,
                Some(FileApi::SetCurrentDirectoryW | FileApi::SetCurrentDirectoryA)
            ) =>
            {
                let path = Self::path_name(mem, args[0], file.unwrap().wide())?;
                match self
                    .files
                    .filesystem_mut()
                    .ok_or("Win64 has no mounted filesystem")?
                    .set_cwd(&path)
                {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            _ if matches!(
                file,
                Some(FileApi::GetCurrentDirectoryW | FileApi::GetCurrentDirectoryA)
            ) =>
            {
                let path = self
                    .files
                    .filesystem()
                    .ok_or("Win64 has no mounted filesystem")?
                    .cwd();
                let wide = file.unwrap().wide();
                let bytes: Vec<u8> = if wide {
                    path.encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect()
                } else {
                    crate::codepage::encode(&(path + "\0"), 932)
                        .map_err(|e| format!("cwd codepage error {e}"))?
                        .0
                };
                let required = bytes.len() / if wide { 2 } else { 1 };
                if args[0] < required as u64 {
                    required as u64
                } else {
                    mem.write(args[1], &bytes)?;
                    required as u64 - 1
                }
            }
            "CommandLineToArgvW" => {
                mem.check(args[1], 4, WRITE)?;
                let line = Self::utf16(mem, args[0])?;
                let words = if line.is_empty() {
                    vec![self.exe_path.clone()]
                } else {
                    command::arguments(&line)
                };
                let strings: Vec<Vec<u8>> = words
                    .iter()
                    .map(|word| {
                        word.encode_utf16()
                            .chain(Some(0))
                            .flat_map(u16::to_le_bytes)
                            .collect()
                    })
                    .collect();
                let table_size = strings.len() * 8;
                let size = table_size + strings.iter().map(Vec::len).sum::<usize>();
                let block = self.heap_alloc(mem, size as u64)?;
                if block == 0 {
                    0
                } else {
                    let mut data = vec![0; size];
                    let mut at = table_size;
                    for (n, string) in strings.iter().enumerate() {
                        data[n * 8..n * 8 + 8].copy_from_slice(&(block + at as u64).to_le_bytes());
                        data[at..at + string.len()].copy_from_slice(string);
                        at += string.len();
                    }
                    mem.write(block, &data)?;
                    mem.write_int(args[1], 4, words.len() as u64)?;
                    self.local_blocks.insert(block);
                    block
                }
            }
            "GetErrorMode" => self.error_mode as u64,
            "SetErrorMode" => {
                if args[0] & !0x8007 != 0 {
                    return Err("unsupported Win64 process error-mode bits".into());
                }
                let previous = self.error_mode;
                self.error_mode = args[0] as u32;
                previous as u64
            }
            "InitializeProcThreadAttributeList" => {
                if args[1] == 0 || args[1] > 16 || args[2] != 0 {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let needed = 32 + args[1] * 32;
                    mem.check(args[3], 8, WRITE)?;
                    let capacity = mem.read_int(args[3], 8)?;
                    if args[0] == 0 || capacity < needed {
                        mem.write_int(args[3], 8, needed)?;
                        self.set_error(mem, 122)?;
                        0
                    } else {
                        mem.check(args[0], needed as usize, WRITE)?;
                        mem.write(args[0], &vec![0; needed as usize])?;
                        self.process_attributes
                            .insert(args[0], (args[1] as u32, BTreeMap::new()));
                        1
                    }
                }
            }
            "UpdateProcThreadAttribute" => {
                if args[1] & !1 != 0 || args[5] != 0 || args[6] != 0 {
                    self.set_error(mem, 87)?;
                    0
                } else if args[2] != 0x20002 {
                    return Err(format!(
                        "unsupported Win64 process attribute {:#x}",
                        args[2]
                    ));
                } else if args[4] == 0 || args[4] > 64 * 8 || args[4] % 8 != 0 {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let mut bytes = vec![0; args[4] as usize];
                    mem.read(args[3], &mut bytes)?;
                    let valid_handles = bytes.as_chunks::<8>().0.iter().all(|b| {
                        let handle = u64::from_le_bytes(*b);
                        self.pipes
                            .flags(handle)
                            .or_else(|| self.events.flags(handle))
                            .is_some_and(|flags| flags & 1 != 0)
                    });
                    if !valid_handles {
                        self.set_error(mem, 6)?;
                        0
                    } else if let Some((capacity, attributes)) =
                        self.process_attributes.get_mut(&args[0])
                    {
                        if attributes.contains_key(&args[2]) && args[1] == 0
                            || !attributes.contains_key(&args[2])
                                && attributes.len() >= *capacity as usize
                        {
                            self.set_error(mem, 87)?;
                            0
                        } else {
                            attributes.insert(args[2], bytes);
                            1
                        }
                    } else {
                        self.set_error(mem, 87)?;
                        0
                    }
                }
            }
            "DeleteProcThreadAttributeList" => {
                if self.process_attributes.remove(&args[0]).is_none() {
                    return Err("invalid Win64 process attribute list".into());
                }
                0
            }
            "CreateProcessW" => {
                // Only a missing unambiguous executable can currently be resolved. A
                // present executable must be executed by a real child-process backend.
                mem.check(args[8], 104, READ)?;
                mem.check(args[9], 24, WRITE)?;
                let cb = mem.read_int(args[8], 4)?;
                if cb != 104 && cb != 112 {
                    self.set_error(mem, 87)?;
                    returned.regs[0] = 0;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, 0);
                    return Ok(Control::Returned);
                }
                let application = if args[0] != 0 {
                    Self::utf16(mem, args[0])?
                } else {
                    let command = Self::utf16(mem, args[1])?;
                    let command = command.trim_start();
                    if let Some(quoted) = command.strip_prefix('"') {
                        quoted
                            .split_once('"')
                            .ok_or("unterminated Win64 child executable quote")?
                            .0
                            .to_owned()
                    } else {
                        let first = command
                            .split_whitespace()
                            .next()
                            .ok_or("empty Win64 child command line")?;
                        if !first.to_ascii_lowercase().ends_with(".exe") {
                            return Err(
                                "Win64 ambiguous unquoted child executable search not implemented"
                                    .into(),
                            );
                        }
                        first.to_owned()
                    }
                };
                let fs = self
                    .files
                    .filesystem()
                    .ok_or("Win64 CreateProcess has no mounted guest filesystem")?;
                let result = fs
                    .resolve(&application)
                    .and_then(|path| std::fs::metadata(path).map_err(|e| crate::fs::win_error(&e)));
                match result {
                    Err(error) => {
                        self.set_error(mem, error)?;
                        self.trace.push_back(format!(
                            "CreateProcessW executable={application:?} failed error={error}"
                        ));
                        if self.trace.len() > 4096 {
                            self.trace.pop_front();
                        }
                        0
                    }
                    Ok(_) => {
                        return Err(format!(
                            "Win64 child-process execution not implemented: {application}"
                        ))
                    }
                }
            }
            "CreatePipe" => {
                mem.check(args[0], 8, WRITE)?;
                mem.check(args[1], 8, WRITE)?;
                let inherit = if args[2] == 0 {
                    false
                } else {
                    mem.check(args[2], 24, READ)?;
                    if mem.read_int(args[2], 4)? != 24 || mem.read_int(args[2] + 8, 8)? != 0 {
                        return Err("Win64 pipe security descriptor not implemented".into());
                    }
                    mem.read_int(args[2] + 16, 4)? != 0
                };
                match self.pipes.create(args[3] as u32, inherit) {
                    Ok((read, write)) => {
                        mem.write_int(args[0], 8, read)?;
                        mem.write_int(args[1], 8, write)?;
                        1
                    }
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
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
                    let mut bytes = vec![0; count];
                    let result = match self.files.read(args[0], &mut bytes) {
                        Ok(read) => {
                            mem.write(args[1], &bytes[..read])?;
                            mem.write_int(args[3], 4, read as u64)?;
                            1
                        }
                        Err(error) => {
                            mem.write_int(args[3], 4, 0)?;
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
                match self.pipes.read(args[0], count) {
                    Ok(Some(bytes)) => {
                        mem.write(args[1], &bytes)?;
                        mem.write_int(args[3], 4, bytes.len() as u64)?;
                        1
                    }
                    Ok(None) => {
                        return Err("Win64 blocking pipe read requires a thread scheduler".into())
                    }
                    Err(error) => {
                        mem.write_int(args[3], 4, 0)?;
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            _ if matches!(binding.control, gateways::ControlImport::Wait(_))
                || name == "WSAWaitForMultipleEvents" =>
            {
                let wait = match binding.control {
                    gateways::ControlImport::Wait(id) => Some(id),
                    _ => None,
                };
                let winsock = wait.is_none();
                if winsock && self.winsock_refs == 0 {
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
            "SetThreadStackGuarantee" => {
                let requested = mem.read_int(args[0], 4)?;
                if requested == 0 {
                    // No overflow/unwind path exists yet, so this thread has no guaranteed
                    // exception-handler reserve. A zero request only queries that state.
                    mem.write_int(args[0], 4, 0)?;
                    1
                } else {
                    self.set_error(mem, 50)?;
                    0
                }
            }
            "EncodePointer" | "DecodePointer" => args[0], // Optional pointer obfuscation; inverse identity.
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
            "AreFileApisANSI" => 1, // This provider uses its declared ANSI code page for file paths.
            "GetTempPathW" | "GetTempPath2W" => {
                // Like Windows, this query does not test or create the directory.
                let path: Vec<u16> = "C:\\Temp\\".encode_utf16().collect();
                if args[0] as usize <= path.len() {
                    (path.len() + 1) as u64
                } else {
                    let bytes: Vec<u8> = path
                        .iter()
                        .copied()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect();
                    mem.write(args[1], &bytes)?;
                    path.len() as u64
                }
            }
            _ if matches!(
                handle,
                Some(HandleApi::GetStdHandle | HandleApi::SetStdHandle)
            ) =>
            {
                let index = match args[0] as u32 {
                    0xfffffff6 => Some(0),
                    0xfffffff5 => Some(1),
                    0xfffffff4 => Some(2),
                    _ => None,
                };
                if let Some(index) = index {
                    if handle == Some(HandleApi::GetStdHandle) {
                        self.files.standard().redirect(index)
                    } else {
                        self.files.standard_mut().set_redirect(index, args[1]);
                        1
                    }
                } else {
                    self.set_error(mem, 87)?;
                    if handle == Some(HandleApi::GetStdHandle) {
                        u64::MAX
                    } else {
                        0
                    }
                }
            }
            _ if file == Some(FileApi::GetFileType)
                || handle == Some(HandleApi::CloseHandle)
                || name == "GetConsoleMode" =>
            {
                let index = self.files.standard().index(args[0]);
                if index.is_some() {
                    match name {
                        _ if file == Some(FileApi::GetFileType) => 3, // Redirected host pipe, not a console.
                        "GetConsoleMode" => {
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
            _ if file == Some(FileApi::WriteFile) || name == "WriteConsoleW" => {
                if file == Some(FileApi::WriteFile) && self.files.flags(args[0]).is_some() {
                    if args[4] != 0 || args[2] > 16 * 1024 * 1024 || args[3] == 0 {
                        return Err(
                            "unsupported Win64 file write overlap/size/result pointer".into()
                        );
                    }
                    mem.check(args[3], 4, WRITE)?;
                    let mut bytes = vec![0; args[2] as usize];
                    mem.read(args[1], &mut bytes)?;
                    let position = self.files.position(args[0]);
                    let metadata = self.files.metadata(args[0]).map(|(m, _)| m);
                    let write = position.and_then(|position| {
                        metadata.and_then(|metadata| {
                            self.files
                                .write(args[0], &bytes)
                                .map(|written| (position, metadata, written))
                        })
                    });
                    let result = match write {
                        Ok((position, metadata, written)) => {
                            self.mappings
                                .update(mem, &metadata, position, &bytes[..written])?;
                            mem.write_int(args[3], 4, written as u64)?;
                            1
                        }
                        Err(error) => {
                            mem.write_int(args[3], 4, 0)?;
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
                if file == Some(FileApi::WriteFile) && self.pipes.flags(args[0]).is_some() {
                    if args[4] != 0 || args[2] > 16 * 1024 * 1024 || args[3] == 0 {
                        return Err(
                            "unsupported Win64 pipe write overlap/size/result pointer".into()
                        );
                    }
                    mem.check(args[3], 4, WRITE)?;
                    let mut bytes = vec![0; args[2] as usize];
                    mem.read(args[1], &mut bytes)?;
                    let value = match self.pipes.write(args[0], &bytes) {
                        Ok(true) => {
                            mem.write_int(args[3], 4, args[2])?;
                            1
                        }
                        Ok(false) => {
                            return Err(
                                "Win64 blocking pipe write requires a thread scheduler".into()
                            )
                        }
                        Err(error) => {
                            mem.write_int(args[3], 4, 0)?;
                            self.set_error(mem, error)?;
                            0
                        }
                    };
                    returned.regs[0] = value;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, value);
                    return Ok(Control::Returned);
                }
                let index = self.files.standard().index(args[0]).filter(|&i| i != 0);
                if name == "WriteConsoleW" || index.is_none() {
                    self.set_error(mem, 6)?;
                    0
                } else if args[4] != 0 {
                    self.set_error(mem, 50)?;
                    0
                } else {
                    use std::io::Write;
                    let count = args[2] as u32 as usize;
                    if count > 16 * 1024 * 1024 {
                        return Err("Win64 stdio transfer exceeds capacity".into());
                    }
                    if args[3] != 0 {
                        mem.check(args[3], 4, WRITE)?;
                    }
                    let mut bytes = vec![0; count];
                    mem.read(args[1], &mut bytes)?;
                    let result = if index == Some(1) {
                        std::io::stdout().lock().write_all(&bytes)
                    } else {
                        std::io::stderr().lock().write_all(&bytes)
                    };
                    match result {
                        Ok(()) => {
                            if args[3] != 0 {
                                mem.write_int(args[3], 4, count as u64)?;
                            }
                            1
                        }
                        Err(error) => {
                            self.set_error(mem, crate::fs::win_error(&error))?;
                            0
                        }
                    }
                }
            }
            "SetUnhandledExceptionFilter" => {
                let previous = self.exception_filter;
                self.exception_filter = args[0];
                previous
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
            "ExitProcess" => {
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
                if index >= self.tls.len() || self.tls[index].is_none() {
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
                    self.tls[index] = if binding.hot == H::TlsFree {
                        None
                    } else {
                        Some(value)
                    };
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
        if self.trace.len() == 4096 {
            self.trace.pop_front();
        }
        self.trace.push_back(message);
    }
}
