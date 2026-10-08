//! Checked Windows x64 startup services. No host pointers enter the guest.
//! This is a bring-up layer, not a complete Windows/Unity runtime.
use super::{
    cpu::Cpu64,
    memory::{Memory, PAGE, READ, WRITE},
};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
mod address;
mod apc;
mod command;
mod display;
mod dxgi;
mod events;
pub(crate) mod exception;
mod files;
mod find;
mod heap;
mod locale;
mod mapping;
mod modules;
mod pipes;
mod registry;
mod resources;
mod snapshot;
mod sockets;
mod sspi;
mod threads;
mod time;
mod timers;
mod token;
mod tracing;
mod unwind;
mod user;
pub use user::WindowCall;
pub(crate) enum WindowStep {
    Next(u64, [u64; 4]),
    Returned(u64),
}
mod version;

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
}
#[derive(Debug, Clone, Copy)]
pub struct WindowEvent {
    pub window: u64,
    pub message: u32,
    pub wparam: u64,
    pub lparam: u64,
}

pub struct Runtime64 {
    start: Instant,
    timer_periods: BTreeMap<u32, u32>,
    image_base: u64,
    next_alloc: u64,
    next_module: u64,
    allocations: BTreeMap<u64, (usize, u64)>,
    guest_heap: heap::Heap,
    tls: [Option<u64>; 64],
    fls: [Option<(u64, u64)>; 128],
    modules: BTreeMap<u64, (String, u32)>,
    pinned_modules: std::collections::BTreeSet<u64>,
    guest_exports: BTreeMap<u64, super::pe::Exports64>,
    guest_paths: BTreeMap<u64, String>,
    guest_images: BTreeMap<u64, (u32, u64)>,
    dll_directory: Option<String>,
    std_handles: [u64; 3],
    std_open: [bool; 3],
    std_flags: [u32; 3],
    std_aliases: BTreeMap<u64, (usize, u32)>,
    next_std_alias: u64,
    tokens: BTreeMap<u64, (u32, u32)>,
    next_token: u64,
    etw: BTreeMap<u64, ([u8; 16], u64, u64)>,
    next_etw: u64,
    function_tables: BTreeMap<u64, unwind::Table>,
    exception_handlers: Vec<(u64, u64)>,
    next_exception_handler: u64,
    environment_blocks: std::collections::BTreeSet<u64>,
    environment: BTreeMap<String, (String, String)>,
    local_blocks: std::collections::BTreeSet<u64>,
    task_blocks: std::collections::BTreeSet<u64>,
    rng_handles: std::collections::BTreeSet<u64>,
    exe_path: String,
    pub gateways: BTreeMap<u64, String>,
    static_tls_count: u32,
    static_tls: Vec<(Vec<u8>, u64, u64)>,
    threads: threads::Threads,
    delay_hooks: BTreeMap<u64, (String, u64, u64)>,
    srw: BTreeMap<u64, (u64, u32)>,
    exception_filter: u64,
    error_mode: u32,
    apartment: Option<u32>,
    com_refs: u32,
    winrt_refs: u32,
    screen_reader: bool,
    snapshots: BTreeMap<u64, snapshot::Snapshot>,
    next_snapshot: u64,
    not_indexed: std::collections::BTreeSet<std::path::PathBuf>,
    pub api_calls: u64,
    /// Most recent calls, bounded independently of the full api_calls counter.
    pub trace: VecDeque<String>,
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
    fs: Option<crate::fs::GuestFs>,
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
fn kernel_export(name: &str) -> bool {
    if user::arity(name).is_some()
        || display::arity(name).is_some()
        || registry::arity(name).is_some()
        || sockets::is_call(name)
        || sspi::arity(name).is_some()
        || tracing::is_call(name)
        || dxgi::arity(name).is_some()
        || (modules::is_query(name) && !name.starts_with("K32"))
        || name.starts_with("BCrypt")
        || name.starts_with("time")
        || matches!(
            name,
            "CommandLineToArgvW"
                | "SHGetKnownFolderPath"
                | "CoTaskMemAlloc"
                | "CoTaskMemFree"
                | "CoCreateInstance"
                | "CoInitialize"
                | "CoInitializeEx"
                | "CoUninitialize"
                | "RoInitialize"
                | "RoUninitialize"
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
                | "SystemParametersInfoA"
                | "SystemParametersInfoW"
                | "GetSystemMetrics"
        )
    {
        return false;
    }
    import_arity(name).is_some()
}
fn import_arity(name: &str) -> Option<usize> {
    Some(match name {
        name if timers::arity(name).is_some() => timers::arity(name).unwrap(),
        name if dxgi::arity(name).is_some() => dxgi::arity(name).unwrap(),
        name if find::arity(name).is_some() => find::arity(name).unwrap(),
        name if user::arity(name).is_some() => user::arity(name).unwrap(),
        name if registry::arity(name).is_some() => registry::arity(name).unwrap(),
        name if display::arity(name).is_some() => display::arity(name).unwrap(),
        name if sspi::arity(name).is_some() => sspi::arity(name).unwrap(),
        "GetSystemDirectoryA"
        | "GetSystemDirectoryW"
        | "GetWindowsDirectoryA"
        | "GetWindowsDirectoryW"
        | "GetSystemWindowsDirectoryA"
        | "GetSystemWindowsDirectoryW" => 2,
        "GetVersion" => 0,
        "RtlGetVersion" | "GetVersionExA" | "GetVersionExW" => 1,
        "RtlVerifyVersionInfo" | "VerifyVersionInfoW" | "VerSetConditionMask" => 3,
        "SetDllDirectoryW" | "SetDllDirectoryA" => 1,
        "GetDllDirectoryW" | "GetDllDirectoryA" => 2,
        "WaitOnAddress" => 4,
        "WakeByAddressSingle" | "WakeByAddressAll" => 1,
        "WSAStartup" => 2,
        "WSASetLastError" => 1,
        "WSACleanup" | "WSAGetLastError" => 0,
        "InitializeCriticalSection" | "TryEnterCriticalSection" => 1,
        "SetCriticalSectionSpinCount" => 2,
        "AddVectoredExceptionHandler" => 2,
        "RemoveVectoredExceptionHandler" => 1,
        "RaiseException" => 4,
        "RtlUnwindEx" => 6,
        "RtlAddFunctionTable" | "RtlLookupFunctionEntry" => 3,
        "RtlDeleteFunctionTable" => 1,
        "RtlInstallFunctionTableCallback" => 6,
        "GetModuleHandleExA" | "GetModuleHandleExW" => 3,
        "EventRegister" | "EventWrite" | "EventWriteString" | "EventSetInformation" => 4,
        "EventUnregister" => 1,
        "EventEnabled" => 2,
        "EventProviderEnabled" => 3,
        "EventWriteTransfer" => 6,
        "FormatMessageA" | "FormatMessageW" => 7,
        "K32EnumProcessModules"
        | "EnumProcessModules"
        | "K32GetModuleInformation"
        | "GetModuleInformation"
        | "K32GetModuleBaseNameA"
        | "K32GetModuleBaseNameW"
        | "GetModuleBaseNameA"
        | "GetModuleBaseNameW"
        | "K32GetModuleFileNameExA"
        | "K32GetModuleFileNameExW"
        | "GetModuleFileNameExA"
        | "GetModuleFileNameExW" => 4,
        "K32EnumProcessModulesEx" | "EnumProcessModulesEx" => 5,
        "GetSidSubAuthority" | "EqualSid" => 2,
        "CopySid" | "InitializeSid" => 3,
        "GetSidSubAuthorityCount"
        | "GetSidIdentifierAuthority"
        | "IsValidSid"
        | "GetLengthSid"
        | "GetSidLengthRequired" => 1,
        "LocalAlloc" | "GlobalAlloc" => 2,
        "LocalReAlloc" | "GlobalReAlloc" => 3,
        "LocalSize" | "GlobalSize" | "LocalLock" | "GlobalLock" | "LocalUnlock"
        | "GlobalUnlock" | "GlobalFree" | "LocalFlags" | "GlobalFlags" => 1,
        "OpenProcessToken" => 3,
        "OpenThreadToken" => 4,
        "GetTokenInformation" => 5,
        "MoveFileExW" | "MoveFileExA" => 3,
        "MoveFileW" | "MoveFileA" => 2,
        "DeleteFileW" | "DeleteFileA" | "RemoveDirectoryW" | "RemoveDirectoryA" => 1,
        "DuplicateHandle" => 7,
        "SetFileAttributesW" | "SetFileAttributesA" => 2,
        "SHGetKnownFolderPath" => 4,
        "CoTaskMemAlloc" | "CoTaskMemFree" => 1,
        "CoCreateInstance" => 5,
        "timeGetTime" => 0,
        "timeBeginPeriod" | "timeEndPeriod" => 1,
        "timeGetDevCaps" => 2,
        "CreateDirectoryW" | "CreateDirectoryA" => 2,
        "GetDateFormatEx" => 7,
        "GetTimeFormatEx" => 6,
        "EnumSystemLocalesEx" | "GetLocaleInfoEx" | "GetLocaleInfoW" | "LCIDToLocaleName" => 4,
        "LocaleNameToLCID" | "GetUserDefaultLocaleName" | "GetSystemDefaultLocaleName" => 2,
        "IsValidLocaleName" => 1,
        "GetUserDefaultLCID"
        | "GetSystemDefaultLCID"
        | "GetThreadLocale"
        | "GetUserDefaultLangID"
        | "GetSystemDefaultLangID" => 0,
        "CreateToolhelp32Snapshot"
        | "Process32First"
        | "Process32Next"
        | "Process32FirstW"
        | "Process32NextW"
        | "Thread32First"
        | "Thread32Next" => 2,
        "SystemParametersInfoA" | "SystemParametersInfoW" => 4,
        "GetSystemMetrics" => 1,
        "CreateThread" => 6,
        "Sleep"
        | "ExitThread"
        | "ResumeThread"
        | "SuspendThread"
        | "GetThreadId"
        | "GetThreadPriority"
        | "DisableThreadLibraryCalls" => 1,
        "SwitchToThread" => 0,
        "QueueUserAPC" => 3,
        "SleepEx" => 2,
        "GetExitCodeThread"
        | "SetThreadPriority"
        | "SetThreadAffinityMask"
        | "SetThreadDescription"
        | "GetThreadDescription"
        | "GetThreadContext" => 2,
        "CreateSemaphoreExW" | "CreateSemaphoreExA" => 6,
        "CreateSemaphoreW" | "CreateSemaphoreA" => 4,
        "OpenSemaphoreW" | "OpenSemaphoreA" | "ReleaseSemaphore" => 3,
        "GetLogicalProcessorInformationEx" => 3,
        "GetActiveProcessorCount" | "GetMaximumProcessorCount" => 1,
        "GetActiveProcessorGroupCount" | "GetMaximumProcessorGroupCount" => 0,
        "BCryptGenRandom" | "BCryptOpenAlgorithmProvider" => 4,
        "BCryptCloseAlgorithmProvider" => 2,
        "GetEnvironmentVariableW" | "GetEnvironmentVariableA" => 3,
        "SetEnvironmentVariableW" | "SetEnvironmentVariableA" => 2,
        "GlobalMemoryStatusEx" => 1,
        "CreateFileW" | "CreateFileA" => 7,
        "SetFilePointerEx" => 4,
        "GetFileSizeEx" | "GetFileSize" => 2,
        "GetFileInformationByHandle" => 2,
        "GetFileInformationByHandleEx" | "GetFileTime" => 4,
        "CreateFileMappingW" | "CreateFileMappingA" | "MapViewOfFileEx" => 6,
        "MapViewOfFile" => 5,
        "UnmapViewOfFile" => 1,
        "FlushViewOfFile" => 2,
        "FlushFileBuffers" => 1,
        "GetFileAttributesW"
        | "GetFileAttributesA"
        | "SetCurrentDirectoryW"
        | "SetCurrentDirectoryA" => 1,
        "GetCurrentDirectoryW" | "GetCurrentDirectoryA" => 2,
        "GetFileAttributesExW" | "GetFileAttributesExA" => 3,
        "CommandLineToArgvW" => 2,
        "LocalFree" => 1,
        "RoInitialize" | "CoInitialize" => 1,
        "CoInitializeEx" => 2,
        "RoUninitialize" | "CoUninitialize" => 0,
        "GetCurrentProcessId"
        | "GetCurrentThreadId"
        | "GetCurrentProcess"
        | "GetCurrentThread"
        | "GetLastError"
        | "GetCommandLineA"
        | "GetCommandLineW"
        | "GetTickCount"
        | "GetTickCount64"
        | "IsDebuggerPresent"
        | "TlsAlloc"
        | "GetProcessHeap"
        | "AreFileApisANSI"
        | "GetACP"
        | "GetOEMCP"
        | "GetConsoleOutputCP"
        | "GetConsoleCP" => 0,
        "GetErrorMode" => 0,
        "SetErrorMode" => 1,
        "GetThreadErrorMode" => 0,
        "SetThreadErrorMode" => 2,
        "GetEnvironmentStringsW" => 0,
        "AcquireSRWLockExclusive"
        | "WakeAllConditionVariable"
        | "WakeConditionVariable"
        | "InitializeConditionVariable"
        | "ReleaseSRWLockExclusive"
        | "AcquireSRWLockShared"
        | "ReleaseSRWLockShared"
        | "TryAcquireSRWLockExclusive"
        | "TryAcquireSRWLockShared"
        | "InitializeSRWLock" => 1,
        "InitOnceBeginInitialize" => 4,
        "InitOnceComplete" => 3,
        "SystemTimeToFileTime" | "FileTimeToSystemTime" | "CompareFileTime" => 2,
        "SystemTimeToTzSpecificLocalTime" | "TzSpecificLocalTimeToSystemTime" => 3,
        "GetTimeZoneInformation" => 1,
        "SetThreadStackGuarantee" => 1,
        "VirtualAlloc" | "VirtualProtect" | "VirtualFreeEx" | "VirtualQueryEx" => 4,
        "VirtualAllocEx" | "VirtualProtectEx" => 5,
        "VirtualFree" | "VirtualQuery" => 3,
        "CreateEventW"
        | "CreateEventA"
        | "CreateEventExW"
        | "CreateEventExA"
        | "WaitForMultipleObjects" => 4,
        "WaitForMultipleObjectsEx" => 5,
        "OpenEventW" | "OpenEventA" | "WaitForSingleObjectEx" => 3,
        "WaitForSingleObject" => 2,
        "SetEvent" | "ResetEvent" => 1,
        "CreatePipe" => 4,
        "GetHandleInformation" => 2,
        "SetHandleInformation" => 3,
        "ReadFile" => 5,
        "InitializeProcThreadAttributeList" => 4,
        "UpdateProcThreadAttribute" => 7,
        "DeleteProcThreadAttributeList" => 1,
        "CreateProcessW" => 10,
        "FlsAlloc"
        | "FlsGetValue"
        | "FlsFree"
        | "GetModuleHandleA"
        | "LoadLibraryW"
        | "LoadLibraryA"
        | "FreeLibrary"
        | "GetStdHandle"
        | "GetFileType"
        | "CloseHandle"
        | "IsValidCodePage"
        | "FreeEnvironmentStringsW" => 1,
        "GetProcAddress" | "FlsSetValue" | "SetStdHandle" | "GetConsoleMode" | "GetCPInfo"
        | "GetTempPathW" | "GetTempPath2W" => 2,
        "LoadLibraryExW" | "LoadLibraryExA" | "GetCPInfoExA" | "GetCPInfoExW" => 3,
        "GetSystemTimeAsFileTime"
        | "GetSystemTimePreciseAsFileTime"
        | "GetSystemTime"
        | "GetLocalTime"
        | "GetSystemInfo"
        | "GetNativeSystemInfo"
        | "QueryPerformanceCounter"
        | "QueryPerformanceFrequency"
        | "InitializeSListHead"
        | "GetStartupInfoW"
        | "GetStartupInfoA"
        | "GetModuleHandleW"
        | "SetLastError"
        | "IsProcessorFeaturePresent"
        | "TlsGetValue"
        | "TlsFree"
        | "EncodePointer"
        | "DecodePointer"
        | "SetUnhandledExceptionFilter"
        | "ExitProcess"
        | "EnterCriticalSection"
        | "LeaveCriticalSection"
        | "DeleteCriticalSection" => 1,
        "TlsSetValue" | "InitializeCriticalSectionAndSpinCount" => 2,
        "HeapAlloc"
        | "HeapFree"
        | "HeapSize"
        | "InitializeCriticalSectionEx"
        | "GetModuleFileNameW"
        | "GetModuleFileNameA" => 3,
        "HeapReAlloc" | "GetStringTypeW" => 4,
        "WriteFile" | "WriteConsoleW" => 5,
        "MultiByteToWideChar" | "LCMapStringW" => 6,
        "WideCharToMultiByte" => 8,
        "LCMapStringEx" => 9,
        _ => return None,
    })
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
            start: Instant::now(),
            timer_periods: BTreeMap::new(),
            image_base,
            next_alloc: 0x400000000,
            next_module: TEB + 0x20000,
            allocations: BTreeMap::new(),
            guest_heap: Default::default(),
            tls: [None; 64],
            fls: [None; 128],
            modules: BTreeMap::new(),
            pinned_modules: std::collections::BTreeSet::new(),
            guest_exports: BTreeMap::new(),
            guest_paths: BTreeMap::new(),
            guest_images: BTreeMap::new(),
            dll_directory: None,
            std_handles: [0x100, 0x101, 0x102],
            std_open: [true; 3],
            std_flags: [0; 3],
            std_aliases: BTreeMap::new(),
            next_std_alias: 0x7fff3000,
            tokens: BTreeMap::new(),
            next_token: 0x7fff4000,
            etw: BTreeMap::new(),
            next_etw: 0x7fff6000,
            function_tables: BTreeMap::new(),
            exception_handlers: Vec::new(),
            next_exception_handler: 0x7fff8000,
            environment_blocks: std::collections::BTreeSet::new(),
            environment: BTreeMap::new(),
            local_blocks: std::collections::BTreeSet::new(),
            task_blocks: std::collections::BTreeSet::new(),
            rng_handles: std::collections::BTreeSet::new(),
            exe_path: command.trim_matches('"').to_owned(),
            gateways: BTreeMap::new(),
            static_tls_count: 0,
            static_tls: Vec::new(),
            threads: threads::Threads::new(stack_size),
            delay_hooks: BTreeMap::new(),
            srw: BTreeMap::new(),
            exception_filter: 0,
            error_mode: 0,
            apartment: None,
            com_refs: 0,
            winrt_refs: 0,
            screen_reader: false,
            snapshots: BTreeMap::new(),
            next_snapshot: 0x7fff1000,
            not_indexed: std::collections::BTreeSet::new(),
            api_calls: 0,
            trace: VecDeque::new(),
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
            fs: None,
            process_attributes: BTreeMap::new(),
        })
    }
    fn set_error(&self, mem: &mut Memory, error: u32) -> Result<(), String> {
        mem.write_int(self.teb() + 0x68, 4, error as u64)
    }
    pub fn set_filesystem(&mut self, fs: crate::fs::GuestFs) {
        self.fs = Some(fs);
    }
    fn std_index(&self, handle: u64) -> Option<usize> {
        self.std_aliases.get(&handle).map(|h| h.0).or_else(|| {
            handle
                .checked_sub(0x100)
                .filter(|&n| n < 3 && self.std_open[n as usize])
                .map(|n| n as usize)
        })
    }
    fn std_flags(&self, handle: u64) -> Option<u32> {
        self.std_aliases
            .get(&handle)
            .map(|h| h.1)
            .or_else(|| self.std_index(handle).map(|n| self.std_flags[n]))
    }
    fn close_std(&mut self, handle: u64) -> Result<(), u32> {
        if self.std_flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        if self.std_aliases.remove(&handle).is_none() {
            self.std_open[(handle - 0x100) as usize] = false;
        }
        Ok(())
    }
    pub(crate) fn prepare_user_profile(&self) -> Result<(), String> {
        let fs = self
            .fs
            .as_ref()
            .ok_or("Win64 user profile requires a filesystem")?;
        for path in [
            "C:\\Users\\wire\\AppData\\Local",
            "C:\\Users\\wire\\AppData\\LocalLow",
            "C:\\Users\\wire\\AppData\\Roaming",
            "C:\\Users\\wire\\Documents",
        ] {
            let host = fs
                .resolve(path)
                .map_err(|e| format!("Win64 user profile path error {e}"))?;
            std::fs::create_dir_all(host).map_err(|e| format!("Win64 user profile: {e}"))?;
        }
        Ok(())
    }
    fn utf16(mem: &Memory, ptr: u64) -> Result<String, String> {
        let mut units = Vec::new();
        for n in 0..32768u64 {
            let u = mem.read_int(ptr.checked_add(n * 2).ok_or("string overflow")?, 2)? as u16;
            if u == 0 {
                return String::from_utf16(&units)
                    .map_err(|_| "invalid Win64 UTF-16 string".into());
            }
            units.push(u);
        }
        Err("unterminated Win64 UTF-16 string".into())
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
        let module = name
            .replace('\\', "/")
            .rsplit('/')
            .next()
            .unwrap_or(name)
            .to_ascii_lowercase();
        let module = if module.contains('.') {
            module
        } else {
            format!("{module}.dll")
        };
        if load && name.contains(['/', '\\']) {
            let path = self.fs.as_ref().and_then(|fs| fs.resolve(name).ok());
            let loaded = self.guest_paths.values().any(|p| {
                path.is_some() && self.fs.as_ref().and_then(|fs| fs.resolve(p).ok()) == path
            });
            // A logical provider is loaded by its canonical guest system32 name;
            // an arbitrary path with the same basename is not that provider.
            let system_provider = self
                .fs
                .as_ref()
                .zip(path.as_ref())
                .is_some_and(|(fs, path)| {
                    (Self::dynamic_provider(&module)
                        || self.modules.iter().any(|(&handle, (name, _))| {
                            name == &module && !self.guest_exports.contains_key(&handle)
                        }))
                        && fs
                            .guest_path(path)
                            .eq_ignore_ascii_case(&format!("C:\\Windows\\System32\\{module}"))
                });
            if !loaded && !system_provider {
                return Err(format!("Win64 guest DLL requires process loader: {name}"));
            }
        }
        let name = module;
        if let Some((&handle, (_, refs))) = self.modules.iter_mut().find(|(_, (n, _))| n == &name) {
            if load {
                *refs = refs.checked_add(1).ok_or("module reference overflow")?;
            }
            return Ok(handle);
        }
        // These API sets route to the actual startup service provider below.
        if !load {
            return Ok(0);
        }
        if !Self::dynamic_provider(&name) {
            return Err(format!("Win64 guest DLL loading not implemented: {name}"));
        }
        let handle = self.next_module;
        self.next_module += 0x10000;
        self.modules.insert(handle, (name, 1));
        Ok(handle)
    }
    fn dynamic_provider(name: &str) -> bool {
        matches!(
            name,
            "kernel32.dll"
                | "kernelbase.dll"
                | "secur32.dll"
                | "dxgi.dll"
                | "d3d12.dll"
                | "d3d11.dll"
                | "winmm.dll"
                | "api-ms-win-core-synch-l1-2-0.dll"
                | "api-ms-win-core-fibers-l1-1-1.dll"
                | "api-ms-win-core-localization-l1-2-1.dll"
                | "api-ms-win-core-string-l1-1-0.dll"
                | "api-ms-win-core-datetime-l1-1-1.dll"
        )
    }
    pub fn install_gateways(&mut self, gateways: BTreeMap<u64, String>) {
        self.gateways = gateways;
        // Kernel32 is an implicit, persistent process module.
        self.native_module("kernel32.dll", true)
            .expect("kernel32 provider");
        self.register_provider("ntdll.dll");
        let ntdll = self
            .modules
            .iter()
            .find(|(_, (name, _))| name == "ntdll.dll")
            .unwrap()
            .0;
        self.pinned_modules.insert(*ntdll);
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
    pub fn register_module(&mut self, name: &str, base: u64, exports: super::pe::Exports64) {
        self.modules.insert(base, (name.to_ascii_lowercase(), 1));
        self.guest_exports.insert(base, exports);
    }
    pub(crate) fn register_module_path(&mut self, base: u64, path: String) {
        self.guest_paths.insert(base, path);
    }
    pub(crate) fn register_image(&mut self, base: u64, size: u32, entry: u64) {
        self.guest_images.insert(base, (size, entry));
    }
    pub(crate) fn filesystem(&self) -> Result<crate::fs::GuestFs, String> {
        self.fs
            .clone()
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
        if self
            .modules
            .values()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            return;
        }
        let handle = self.next_module;
        self.next_module += 0x10000;
        self.modules.insert(handle, (name.to_ascii_lowercase(), 1));
    }
    pub fn register(&mut self, dll: &str, name: &str) -> u64 {
        let name = dxgi::symbol(dll, sockets::symbol(dll, name));
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
        let count = count as u32 as i32;
        if count == 0 || count < -1 || ptr == 0 {
            return Err("invalid Win64 string input".into());
        }
        let length = if count == -1 {
            let mut n = 0;
            loop {
                if n >= 32768 {
                    return Err("Win64 terminated string exceeds capacity".into());
                }
                let unit = mem.read_int(
                    ptr.checked_add(n as u64 * width as u64)
                        .ok_or("string overflow")?,
                    width,
                )?;
                n += 1;
                if unit == 0 {
                    break n;
                }
            }
        } else {
            count as usize
        };
        let size = length
            .checked_mul(width)
            .filter(|&n| n <= 16 * 1024 * 1024)
            .ok_or("Win64 string transfer exceeds capacity")?;
        let mut bytes = vec![0; size];
        mem.read(ptr, &mut bytes)?;
        Ok(bytes)
    }
    fn days_before_year(year: u32) -> u64 {
        let y = year as u64 - 1;
        y * 365 + y / 4 - y / 100 + y / 400
    }
    fn month_days(year: u32, month: u32) -> u32 {
        match month {
            2 => {
                28 + (year.is_multiple_of(4)
                    && (!year.is_multiple_of(100) || year.is_multiple_of(400)))
                    as u32
            }
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }
    fn calendar_to_ticks(fields: [u16; 8]) -> Option<u64> {
        let [year, month, _, day, hour, minute, second, millis] = fields.map(u32::from);
        if !(1601..=30827).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > Self::month_days(year, month)
            || hour > 23
            || minute > 59
            || second > 59
            || millis > 999
        {
            return None;
        }
        let days = Self::days_before_year(year) - Self::days_before_year(1601)
            + (1..month)
                .map(|m| Self::month_days(year, m) as u64)
                .sum::<u64>()
            + day as u64
            - 1;
        Some(
            (days * 86400 + hour as u64 * 3600 + minute as u64 * 60 + second as u64) * 10_000_000
                + millis as u64 * 10000,
        )
    }
    fn ticks_to_calendar(ticks: u64) -> Option<[u16; 8]> {
        let day = ticks / 864000000000;
        let origin = Self::days_before_year(1601);
        if ticks >= 1 << 63 || day >= Self::days_before_year(30828) - origin {
            return None;
        }
        let (mut low, mut high) = (1601, 30828);
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            if Self::days_before_year(mid) - origin <= day {
                low = mid;
            } else {
                high = mid;
            }
        }
        let mut remaining = day - (Self::days_before_year(low) - origin);
        let mut month = 1;
        while remaining >= Self::month_days(low, month) as u64 {
            remaining -= Self::month_days(low, month) as u64;
            month += 1;
        }
        let seconds = (ticks / 10_000_000) % 86400;
        Some([
            low as u16,
            month as u16,
            ((day + 1) % 7) as u16,
            remaining as u16 + 1,
            (seconds / 3600) as u16,
            ((seconds / 60) % 60) as u16,
            (seconds % 60) as u16,
            ((ticks / 10000) % 1000) as u16,
        ])
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
    pub fn dispatch(
        &mut self,
        dll: &str,
        name: &str,
        cpu: &mut Cpu64,
        mem: &mut Memory,
    ) -> Result<Control, String> {
        let name = dxgi::symbol(dll, sockets::symbol(dll, name));
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
            && !(dll.eq_ignore_ascii_case("user32.dll")
                && (display::arity(name).is_some()
                    || user::arity(name).is_some()
                    || matches!(
                        name,
                        "SystemParametersInfoA" | "SystemParametersInfoW" | "GetSystemMetrics"
                    )))
            && !(dll.eq_ignore_ascii_case("psapi.dll") && modules::is_query(name))
            && !(dll.eq_ignore_ascii_case("imm32.dll") && user::ime_arity(name).is_some())
            && !(dll.eq_ignore_ascii_case("gdi32.dll") && user::gdi_arity(name).is_some())
            && !(dll.eq_ignore_ascii_case("advapi32.dll") && tracing::is_call(name))
            && !(dll.eq_ignore_ascii_case("advapi32.dll") && registry::arity(name).is_some())
            && !(dll.eq_ignore_ascii_case("ntdll.dll") && unwind::is_call(name))
            && !(dll.eq_ignore_ascii_case("ntdll.dll") && version::is_call(name))
            && !(dll.eq_ignore_ascii_case("ws2_32.dll") && sockets::is_call(name))
            && !(dll.eq_ignore_ascii_case("secur32.dll") && sspi::arity(name).is_some())
            && !(dll.eq_ignore_ascii_case("winmm.dll")
                && matches!(
                    name,
                    "timeGetTime" | "timeBeginPeriod" | "timeEndPeriod" | "timeGetDevCaps"
                ))
            && !(dll.eq_ignore_ascii_case("dxgi.dll") && dxgi::export(name))
            && !(dll.eq_ignore_ascii_case("d3d12.dll") && dxgi::d3d12_export(name))
            && !(dll.eq_ignore_ascii_case("d3d11.dll") && dxgi::d3d11_export(name))
            && !(dll == dxgi::DLL && dxgi::arity(name).is_some())
            && !(dll.eq_ignore_ascii_case("api-ms-win-core-synch-l1-2-0.dll")
                && matches!(
                    name,
                    "WaitOnAddress" | "WakeByAddressSingle" | "WakeByAddressAll"
                ))
            && !(dll.eq_ignore_ascii_case("bcrypt.dll")
                && matches!(
                    name,
                    "BCryptGenRandom"
                        | "BCryptOpenAlgorithmProvider"
                        | "BCryptCloseAlgorithmProvider"
                ))
            && !(matches!(
                dll.to_ascii_lowercase().as_str(),
                "ole32.dll" | "combase.dll" | "api-ms-win-core-winrt-l1-1-0.dll"
            ) && matches!(
                name,
                "RoInitialize"
                    | "RoUninitialize"
                    | "CoInitialize"
                    | "CoInitializeEx"
                    | "CoUninitialize"
                    | "CoCreateInstance"
                    | "CoTaskMemAlloc"
                    | "CoTaskMemFree"
            ))
        {
            return Err(format!("unimplemented Win64 import: {dll}!{name}"));
        }
        let arity = import_arity(name)
            .ok_or_else(|| format!("unimplemented Win64 import: {dll}!{name}"))?;
        let mut args = cpu.arguments(mem, arity)?;
        let dwords: &[usize] = match name {
            "GetCPInfoExA" | "GetCPInfoExW" => &[0, 1],
            name if dxgi::arity(name).is_some() => dxgi::dwords(name),
            "FindFirstFileExA" | "FindFirstFileExW" => &[1, 3, 5],
            name if user::arity(name).is_some() => user::dwords(name),
            name if registry::arity(name).is_some() => registry::dwords(name),
            name if display::arity(name).is_some() => display::dwords(name),
            name if sspi::arity(name).is_some() => sspi::dwords(name),
            "GetSystemDirectoryA"
            | "GetSystemDirectoryW"
            | "GetWindowsDirectoryA"
            | "GetWindowsDirectoryW"
            | "GetSystemWindowsDirectoryA"
            | "GetSystemWindowsDirectoryW" => &[1],
            "RtlVerifyVersionInfo" | "VerifyVersionInfoW" | "VerSetConditionMask" => &[1],
            "GetDllDirectoryW" | "GetDllDirectoryA" => &[0],
            "WaitOnAddress" => &[3],
            "WSASetLastError" => &[0],
            "SetCriticalSectionSpinCount" => &[1],
            "AddVectoredExceptionHandler" => &[0],
            "RaiseException" => &[0, 1, 2],
            "RtlAddFunctionTable" => &[1],
            "RtlInstallFunctionTableCallback" => &[2],
            "GetModuleHandleExA" | "GetModuleHandleExW" => &[0],
            "EventWrite" => &[2],
            "EventWriteTransfer" => &[4],
            "EventSetInformation" => &[1, 3],
            "FormatMessageA" | "FormatMessageW" => &[0, 2, 3, 5],
            "K32EnumProcessModules" | "EnumProcessModules" => &[2],
            "K32EnumProcessModulesEx" | "EnumProcessModulesEx" => &[2, 4],
            "K32GetModuleInformation" | "GetModuleInformation" => &[3],
            "K32GetModuleBaseNameA"
            | "K32GetModuleBaseNameW"
            | "GetModuleBaseNameA"
            | "GetModuleBaseNameW"
            | "K32GetModuleFileNameExA"
            | "K32GetModuleFileNameExW"
            | "GetModuleFileNameExA"
            | "GetModuleFileNameExW" => &[3],
            "GetSidSubAuthority" => &[1],
            "CopySid" => &[0],
            "LocalAlloc" | "GlobalAlloc" => &[0],
            "LocalReAlloc" | "GlobalReAlloc" => &[2],
            "OpenProcessToken" => &[1],
            "OpenThreadToken" => &[1, 2],
            "GetTokenInformation" => &[1, 3],
            "MoveFileExW" | "MoveFileExA" => &[2],
            "DuplicateHandle" => &[4, 5, 6],
            "SetFileAttributesW" | "SetFileAttributesA" => &[1],
            "SHGetKnownFolderPath" => &[1],
            "GetDateFormatEx" | "GetTimeFormatEx" => &[1, 5],
            "EnumSystemLocalesEx" => &[1],
            "GetLocaleInfoEx" => &[1, 3],
            "GetLocaleInfoW" => &[0, 1, 3],
            "LCIDToLocaleName" => &[0, 2, 3],
            "LocaleNameToLCID" | "GetUserDefaultLocaleName" | "GetSystemDefaultLocaleName" => &[1],
            "CreateToolhelp32Snapshot" => &[0, 1],
            "SystemParametersInfoA" | "SystemParametersInfoW" => &[0, 1, 3],
            "GetSystemMetrics" => &[0],
            "CreateThread" => &[4],
            "Sleep" | "ExitThread" => &[0],
            "SetThreadPriority" => &[1],
            "CreateSemaphoreExW" | "CreateSemaphoreExA" => &[1, 2, 4, 5],
            "CreateSemaphoreW" | "CreateSemaphoreA" => &[1, 2],
            "OpenSemaphoreW" | "OpenSemaphoreA" => &[0, 1],
            "ReleaseSemaphore" => &[1],
            "GetLogicalProcessorInformationEx" => &[0],
            "BCryptGenRandom" => &[2, 3],
            "BCryptOpenAlgorithmProvider" => &[3],
            "BCryptCloseAlgorithmProvider" => &[1],
            "GetEnvironmentVariableW" | "GetEnvironmentVariableA" => &[2],
            "CreateFileW" | "CreateFileA" => &[1, 2, 4, 5],
            "SetFilePointerEx" => &[3],
            "GetFileInformationByHandleEx" => &[1, 3],
            "CreateFileMappingW" | "CreateFileMappingA" => &[2, 3, 4],
            "MapViewOfFile" | "MapViewOfFileEx" => &[1, 2, 3],
            "GetCurrentDirectoryW" | "GetCurrentDirectoryA" => &[0],
            "GetFileAttributesExW" | "GetFileAttributesExA" => &[1],
            "RoInitialize" => &[0],
            "CoInitializeEx" => &[1],
            "CoCreateInstance" => &[2],
            "timeBeginPeriod" | "timeEndPeriod" => &[0],
            "timeGetDevCaps" => &[1],
            name if timers::arity(name).is_some() => timers::dwords(name),
            "SetErrorMode" | "SetThreadErrorMode" => &[0],
            "InitializeProcThreadAttributeList" => &[1, 2],
            "UpdateProcThreadAttribute" => &[1],
            "CreateProcessW" => &[4, 5],
            "CreatePipe" => &[3],
            "SetHandleInformation" => &[1, 2],
            "ReadFile" => &[2],
            "CreateEventW" | "CreateEventA" => &[1, 2],
            "CreateEventExW" | "CreateEventExA" => &[2, 3],
            "OpenEventW" | "OpenEventA" => &[0, 1],
            "WaitForSingleObject" => &[1],
            "WaitForSingleObjectEx" => &[1, 2],
            "SleepEx" => &[0, 1],
            "WaitForMultipleObjects" => &[0, 2, 3],
            "WaitForMultipleObjectsEx" => &[0, 2, 3, 4],
            "VirtualAlloc" => &[2, 3],
            "VirtualProtect" | "VirtualFree" => &[2],
            "VirtualAllocEx" => &[3, 4],
            "VirtualProtectEx" | "VirtualFreeEx" => &[3],
            "SetLastError"
            | "GetTempPathW"
            | "GetTempPath2W"
            | "IsProcessorFeaturePresent"
            | "TlsGetValue"
            | "TlsSetValue"
            | "TlsFree"
            | "FlsGetValue"
            | "FlsSetValue"
            | "FlsFree"
            | "GetStdHandle"
            | "SetStdHandle"
            | "IsValidCodePage"
            | "GetCPInfo"
            | "ExitProcess" => &[0],
            "HeapAlloc"
            | "HeapFree"
            | "HeapSize"
            | "HeapReAlloc"
            | "InitializeCriticalSectionAndSpinCount" => &[1],
            "InitializeCriticalSectionEx" => &[1, 2],
            "LoadLibraryExW" | "LoadLibraryExA" | "GetModuleFileNameW" | "GetModuleFileNameA"
            | "WriteFile" | "WriteConsoleW" => &[2],
            "MultiByteToWideChar" | "WideCharToMultiByte" => &[0, 1, 3, 5],
            "LCMapStringW" => &[0, 1, 3, 5],
            "LCMapStringEx" => &[1, 3, 5],
            "GetStringTypeW" => &[0, 2],
            "InitOnceBeginInitialize" | "InitOnceComplete" => &[1],
            _ => &[],
        };
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
        let ret = match name {
            name if dxgi::arity(name).is_some() => self.dxgi_call(mem, name, &args)?,
            name if find::arity(name).is_some() => self.find_call(mem, name, &args)?,
            name if user::is_callback(name) => {
                match self.window_start(mem, name, &args, &returned)? {
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
            name if user::arity(name).is_some() => self.user_call(mem, name, &args)?,
            name if registry::arity(name).is_some() => self.registry_call(mem, name, &args)?,
            "EnumDisplayMonitors" => {
                let enumeration = self.enumerate_monitors(mem, &args, &returned)?;
                if enumeration.calls.is_empty() {
                    enumeration.result
                } else {
                    returned.regs[0] = enumeration.result;
                    *cpu = returned;
                    self.unblock();
                    self.api_calls += 1;
                    self.record(dll, name, &args, enumeration.result);
                    return Ok(Control::Callbacks {
                        calls: enumeration.calls,
                        release: enumeration.release,
                        stop_on_false: true,
                        return_boolean: true,
                        window_call: None,
                    });
                }
            }
            name if display::arity(name).is_some() => self.display_call(mem, name, &args)?,
            name if sspi::arity(name).is_some() => self.security_call(mem, name, &args)?,
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
            "GetVersion"
            | "RtlGetVersion"
            | "GetVersionExA"
            | "GetVersionExW"
            | "RtlVerifyVersionInfo"
            | "VerifyVersionInfoW"
            | "VerSetConditionMask" => self.version_call(mem, name, &args)?,
            "SetDllDirectoryW" | "SetDllDirectoryA" | "GetDllDirectoryW" | "GetDllDirectoryA" => {
                self.dll_directory_call(mem, name, &args)?
            }
            "WaitOnAddress" => match self.wait_address(mem, &args)? {
                Some(value) => value,
                None => return Ok(Control::Blocked),
            },
            "WakeByAddressSingle" | "WakeByAddressAll" => {
                self.wake_address(args[0], name == "WakeByAddressAll");
                0
            }
            "WSAStartup" | "WSACleanup" | "WSAGetLastError" | "WSASetLastError" => {
                self.winsock_call(mem, name, &args)?
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
                    args,
                    caller: Box::new(returned),
                });
            }
            "RtlUnwindEx" => {
                return Ok(Control::Unwind {
                    dll: dll.into(),
                    args,
                })
            }
            name if unwind::is_call(name) => match self.function_table_call(mem, name, &args)? {
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
                        args,
                    });
                }
            },
            "GetModuleHandleExA" | "GetModuleHandleExW" => {
                self.module_handle_ex(mem, name, &args)?
            }
            name if tracing::is_call(name) => self.etw_call(mem, name, &args)?,
            "FormatMessageA" | "FormatMessageW" => self.format_message(mem, name, &args)?,
            name if modules::is_query(name) => self.module_query(mem, name, &args)?,
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
            "LocalAlloc" | "GlobalAlloc" => {
                if args[0] & !0x40 != 0 {
                    self.set_error(mem, 50)?;
                    0
                } else {
                    let pointer = self.heap_alloc(mem, args[1])?;
                    if pointer != 0 {
                        self.local_blocks.insert(pointer);
                    }
                    pointer
                }
            }
            "LocalSize" | "GlobalSize" | "LocalLock" | "GlobalLock" | "LocalFlags"
            | "GlobalFlags" | "LocalUnlock" | "GlobalUnlock" => {
                if let Some(&(size, _)) = self
                    .allocations
                    .get(&args[0])
                    .filter(|_| self.local_blocks.contains(&args[0]))
                {
                    if name.ends_with("Size") {
                        size as u64
                    } else if name.ends_with("Unlock") {
                        self.set_error(mem, 158)?;
                        0
                    } else if name.ends_with("Lock") {
                        args[0]
                    } else {
                        0
                    }
                } else {
                    self.set_error(mem, 6)?;
                    if name.ends_with("Flags") {
                        0x8000
                    } else {
                        0
                    }
                }
            }
            "LocalReAlloc" | "GlobalReAlloc" => {
                if args[2] & !0x42 != 0 {
                    self.set_error(mem, 50)?;
                    0
                } else if let Some(&(old_size, span)) = self
                    .allocations
                    .get(&args[0])
                    .filter(|_| self.local_blocks.contains(&args[0]))
                {
                    if args[1] > 16 * 1024 * 1024 {
                        self.set_error(mem, 8)?;
                        0
                    } else if args[1] <= span {
                        if args[2] & 0x40 != 0 && args[1] > old_size as u64 {
                            mem.write(
                                args[0] + old_size as u64,
                                &vec![0; args[1] as usize - old_size],
                            )?;
                        }
                        self.allocations.insert(args[0], (args[1] as usize, span));
                        args[0]
                    } else if args[2] & 2 == 0 {
                        self.set_error(mem, 8)?;
                        0
                    } else {
                        let mut data = vec![0; old_size.min(args[1] as usize)];
                        mem.read(args[0], &mut data)?;
                        let pointer = self.heap_alloc(mem, args[1])?;
                        if pointer != 0 {
                            mem.write(pointer, &data)?;
                            mem.unmap(args[0], span)?;
                            self.allocations.remove(&args[0]);
                            self.local_blocks.remove(&args[0]);
                            self.local_blocks.insert(pointer);
                        }
                        pointer
                    }
                } else {
                    self.set_error(mem, 6)?;
                    0
                }
            }
            "OpenProcessToken" => {
                if args[0] != u64::MAX {
                    self.set_error(mem, 6)?;
                    0
                } else if args[1] & !8 != 0 {
                    self.set_error(mem, 50)?;
                    0
                } else if self.tokens.len() >= 128 || self.next_token >= 0x7fff5000 {
                    self.set_error(mem, 8)?;
                    0
                } else {
                    mem.check(args[2], 8, WRITE)?;
                    let handle = self.next_token;
                    mem.write_int(args[2], 8, handle)?;
                    self.next_token += 1;
                    self.tokens.insert(handle, (args[1] as u32, 0));
                    1
                }
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
                if !self.tokens.contains_key(&args[0]) {
                    self.set_error(mem, 6)?;
                    0
                } else if self.tokens[&args[0]].0 & 8 == 0 {
                    self.set_error(mem, 5)?;
                    0
                } else {
                    let data = token::information(args[1] as u32, args[2]);
                    match data {
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                        Ok(data) => {
                            mem.check(args[4], 4, WRITE)?;
                            if args[3] < data.len() as u64 {
                                mem.write_int(args[4], 4, data.len() as u64)?;
                                self.set_error(mem, 122)?;
                                0
                            } else {
                                mem.check(args[2], data.len(), WRITE)?;
                                mem.write(args[2], &data)?;
                                mem.write_int(args[4], 4, data.len() as u64)?;
                                1
                            }
                        }
                    }
                }
            }
            "MoveFileExW" | "MoveFileExA" | "MoveFileW" | "MoveFileA" | "DeleteFileW"
            | "DeleteFileA" | "RemoveDirectoryW" | "RemoveDirectoryA" => {
                let path = Self::path_name(mem, args[0], name.ends_with('W'))?;
                let moving = name.starts_with("Move");
                let destination = if moving && args[1] != 0 {
                    Some(Self::path_name(mem, args[1], name.ends_with('W'))?)
                } else {
                    None
                };
                let result = (|| -> Result<(), u32> {
                    let flags = if moving && name.contains("Ex") {
                        args[2]
                    } else {
                        0
                    };
                    if flags & !1 != 0 || moving && destination.is_none() {
                        return Err(50);
                    }
                    let fs = self.fs.as_ref().ok_or(3u32)?;
                    let source = fs.resolve(&path)?;
                    let current = fs.resolve(&fs.cwd())?;
                    if current.starts_with(&source) {
                        return Err(32);
                    }
                    if self.files.in_use(&source) {
                        return Err(32);
                    }
                    let canonical_source = source
                        .canonicalize()
                        .map_err(|e| crate::fs::win_error(&e))?;
                    if moving {
                        let target = fs.resolve(destination.as_ref().unwrap())?;
                        let meta =
                            std::fs::metadata(&source).map_err(|e| crate::fs::win_error(&e))?;
                        if source == target {
                            return Ok(());
                        }
                        if self.files.in_use(&target) {
                            return Err(32);
                        }
                        if target.exists() {
                            if flags & 1 == 0 {
                                return Err(183);
                            }
                            if target.is_dir() || meta.is_dir() {
                                return Err(5);
                            }
                            if target
                                .metadata()
                                .map_err(|e| crate::fs::win_error(&e))?
                                .permissions()
                                .readonly()
                            {
                                return Err(5);
                            }
                        }
                        if !target.parent().is_some_and(|p| p.is_dir()) {
                            return Err(3);
                        }
                        std::fs::rename(&source, &target).map_err(|e| crate::fs::win_error(&e))?;
                        let canonical_target = target
                            .canonicalize()
                            .map_err(|e| crate::fs::win_error(&e))?;
                        let moved: Vec<_> = self
                            .not_indexed
                            .iter()
                            .filter_map(|p| {
                                p.strip_prefix(&canonical_source)
                                    .ok()
                                    .map(|suffix| (p.clone(), canonical_target.join(suffix)))
                            })
                            .collect();
                        self.not_indexed
                            .retain(|p| !p.starts_with(&canonical_target));
                        for (old, new) in moved {
                            self.not_indexed.remove(&old);
                            self.not_indexed.insert(new);
                        }
                    } else if name.starts_with("RemoveDirectory") {
                        if !source.is_dir() {
                            return Err(267);
                        }
                        std::fs::remove_dir(source).map_err(|e| {
                            if e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                                145
                            } else {
                                crate::fs::win_error(&e)
                            }
                        })?;
                        self.not_indexed.remove(&canonical_source);
                    } else {
                        if source.is_dir() {
                            return Err(5);
                        }
                        if source
                            .metadata()
                            .map_err(|e| crate::fs::win_error(&e))?
                            .permissions()
                            .readonly()
                        {
                            return Err(5);
                        }
                        std::fs::remove_file(source).map_err(|e| crate::fs::win_error(&e))?;
                        self.not_indexed.remove(&canonical_source);
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "DuplicateHandle" => {
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
                    if let Some(index) = self.std_index(args[1]) {
                        if args[6] & 1 != 0 && self.std_flags(args[1]).unwrap() & 2 != 0 {
                            return Err(6);
                        }
                        if let Some(access) = desired {
                            let allowed = if index == 0 { 0x80120089 } else { 0x40120116 };
                            if access & !allowed != 0 {
                                return Err(5);
                            }
                            // A narrowed stdio handle needs separate data access rights.
                            return Err(50);
                        }
                        if self.std_aliases.len() >= 128 || self.next_std_alias >= 0x7fff4000 {
                            return Err(8);
                        }
                        let handle = self.next_std_alias;
                        self.next_std_alias += 1;
                        self.std_aliases
                            .insert(handle, (index, (args[5] != 0) as u32));
                        Ok(handle)
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
                        || self.snapshots.contains_key(&args[1])
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
                            if self.std_index(args[1]).is_some() {
                                self.close_std(args[1]).map_err(|e| {
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
            "SetFileAttributesW" | "SetFileAttributesA" => {
                let path = Self::path_name(mem, args[0], name.ends_with('W'))?;
                let result = (|| -> Result<(), u32> {
                    if args[1] & !0x2090 != 0 {
                        return Err(50);
                    }
                    let path = self.fs.as_ref().ok_or(3u32)?.resolve(&path)?;
                    let path = path.canonicalize().map_err(|e| crate::fs::win_error(&e))?;
                    // Guest content indexing is absent; retain this actual guest metadata
                    // for both queries. Directory/normal flags do not change file type.
                    if args[1] & 0x2000 != 0 {
                        self.not_indexed.insert(path);
                    } else {
                        self.not_indexed.remove(&path);
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "CoTaskMemAlloc" => {
                let p = self.heap_alloc(mem, args[0])?;
                if p != 0 {
                    self.task_blocks.insert(p);
                }
                p
            }
            "CoTaskMemFree" => {
                if args[0] != 0 {
                    if !self.task_blocks.contains(&args[0]) {
                        return Err("invalid Win64 task allocation".into());
                    }
                    let &(_, span) = self
                        .allocations
                        .get(&args[0])
                        .ok_or("released Win64 task allocation")?;
                    mem.unmap(args[0], span)?;
                    self.allocations.remove(&args[0]);
                    self.task_blocks.remove(&args[0]);
                }
                0
            }
            "SHGetKnownFolderPath" => {
                mem.check(args[3], 8, WRITE)?;
                let mut guid = [0; 16];
                mem.read(args[0], &mut guid)?;
                let path = match guid {
                    [0xa4, 0xa1, 0x20, 0xa5, 0x80, 0x17, 0xf6, 0x4f, 0xbd, 0x18, 0x16, 0x73, 0x43, 0xc5, 0xaf, 0x16] => {
                        Some("C:\\Users\\wire\\AppData\\LocalLow")
                    }
                    [0x85, 0x27, 0xb3, 0xf1, 0xba, 0x6f, 0xcf, 0x4f, 0x9d, 0x55, 0x7b, 0x8e, 0x7f, 0x15, 0x70, 0x91] => {
                        Some("C:\\Users\\wire\\AppData\\Local")
                    }
                    [0xdb, 0x85, 0xb6, 0x3e, 0xf9, 0x65, 0xf6, 0x4c, 0xa0, 0x3a, 0xe3, 0xef, 0x65, 0x72, 0x9f, 0x3d] => {
                        Some("C:\\Users\\wire\\AppData\\Roaming")
                    }
                    [0x8f, 0x85, 0x6c, 0x5e, 0x22, 0x0e, 0x60, 0x47, 0x9a, 0xfe, 0xea, 0x33, 0x17, 0xb6, 0x71, 0x73] => {
                        Some("C:\\Users\\wire")
                    }
                    [0xd0, 0x9a, 0xd3, 0xfd, 0x8f, 0x23, 0xaf, 0x46, 0xad, 0xb4, 0x6c, 0x85, 0x48, 0x03, 0x69, 0xc7] => {
                        Some("C:\\Users\\wire\\Documents")
                    }
                    _ => None,
                };
                let data = (|| -> Result<Vec<u8>, u32> {
                    if args[2] != 0 || args[1] & !0xc000 != 0 {
                        return Err(50);
                    }
                    let path = path.ok_or(50u32)?;
                    let host = self.fs.as_ref().ok_or(3u32)?.resolve(path)?;
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
                            self.task_blocks.insert(p);
                            0
                        }
                    }
                }
            }
            "CreateDirectoryW" | "CreateDirectoryA" => {
                if args[1] != 0 {
                    return Err("Win64 directory security attributes not implemented".into());
                }
                let path = Self::path_name(mem, args[0], name.ends_with('W'))?;
                let result = self
                    .fs
                    .as_ref()
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
            "GetDateFormatEx" | "GetTimeFormatEx" => self.format_calendar(mem, name, &args)?,
            "GetUserDefaultLCID"
            | "GetSystemDefaultLCID"
            | "GetThreadLocale"
            | "GetUserDefaultLangID"
            | "GetSystemDefaultLangID" => 0x411,
            "IsValidLocaleName" => {
                (args[0] == 0 || locale::id(&Self::utf16(mem, args[0])?).is_some()) as u64
            }
            "LocaleNameToLCID" => {
                if args[1] & !0x08000000 != 0 {
                    self.set_error(mem, 1004)?;
                    0
                } else {
                    let id = if args[0] == 0 {
                        Some(0x411)
                    } else {
                        locale::id(&Self::utf16(mem, args[0])?)
                    };
                    match id {
                        Some(9) if args[1] == 0 => 0x409,
                        Some(0x11) if args[1] == 0 => 0x411,
                        Some(n) => n as u64,
                        None => {
                            self.set_error(mem, 87)?;
                            0
                        }
                    }
                }
            }
            "LCIDToLocaleName" | "GetUserDefaultLocaleName" | "GetSystemDefaultLocaleName" => {
                let (name, pointer, capacity) = if name == "LCIDToLocaleName" {
                    (
                        if args[3] & !0x08000000 == 0 {
                            locale::name(args[0] as u32)
                        } else {
                            None
                        },
                        args[1],
                        args[2],
                    )
                } else {
                    (Some("ja-JP"), args[0], args[1])
                };
                if let Some(name) = name {
                    let data: Vec<_> = name
                        .encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect();
                    if capacity != 0 && (capacity as i32 <= 0 || capacity < data.len() as u64 / 2) {
                        self.set_error(mem, 122)?;
                        0
                    } else {
                        if capacity != 0 {
                            mem.write(pointer, &data)?;
                        }
                        data.len() as u64 / 2
                    }
                } else {
                    self.set_error(mem, 87)?;
                    0
                }
            }
            "GetLocaleInfoEx" | "GetLocaleInfoW" => {
                let id = if name == "GetLocaleInfoW" {
                    locale::name(args[0] as u32).and_then(locale::id)
                } else if args[0] == 0 {
                    Some(0x411)
                } else {
                    locale::id(&Self::utf16(mem, args[0])?)
                };
                let data = if args[1] & !(0x80000000 | 0x20000000 | 0xffff) != 0 {
                    Err(1004)
                } else {
                    id.ok_or(87)
                        .and_then(|id| locale::info(id, args[1] as u32 & !0x80000000))
                };
                match data {
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                    Ok(data) => {
                        let required = data.len() as u64 / 2;
                        if args[3] != 0 && (args[3] as i32 <= 0 || args[3] < required) {
                            self.set_error(mem, 122)?;
                            0
                        } else {
                            if args[3] != 0 {
                                mem.write(args[2], &data)?;
                            }
                            required
                        }
                    }
                }
            }
            "EnumSystemLocalesEx" => {
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
                    let mut calls = Vec::new();
                    let block = self.heap_alloc(mem, 64)?;
                    if block == 0 {
                        0
                    } else {
                        let mut at = block;
                        for (locale, flags) in
                            [("en-US", 0x21), ("ja-JP", 0x21), ("en", 0x11), ("ja", 0x11)]
                        {
                            let requested = args[1] as u32;
                            if requested != 0 && requested & 7 != 0 && requested & 1 == 0 {
                                continue;
                            }
                            if requested & 0x30 != 0 && requested & flags & 0x30 == 0 {
                                continue;
                            }
                            let data: Vec<_> = locale
                                .encode_utf16()
                                .chain(Some(0))
                                .flat_map(u16::to_le_bytes)
                                .collect();
                            mem.write(at, &data)?;
                            calls.push((args[0], [at, flags as u64, args[2], 0]));
                            at += data.len() as u64;
                        }
                        returned.regs[0] = 1;
                        *cpu = returned;
                        self.unblock();
                        self.api_calls += 1;
                        self.record(dll, name, &args, 1);
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
                if args[0] & 6 == 0 || args[0] & !0x80000006 != 0 {
                    self.set_error(mem, 50)?;
                    u64::MAX
                } else if self.snapshots.len() >= 128 || self.next_snapshot >= 0x7fff2000 {
                    self.set_error(mem, 8)?;
                    u64::MAX
                } else {
                    let handle = self.next_snapshot;
                    let threads = self
                        .threads
                        .entries
                        .iter()
                        .filter(|(_, t)| t.exit.is_none())
                        .map(|(&id, t)| (id as u32, t.priority))
                        .collect();
                    self.snapshots.insert(
                        handle,
                        snapshot::Snapshot {
                            process: args[0] & 2 != 0,
                            enumerate_threads: args[0] & 4 != 0,
                            threads,
                            name: self
                                .exe_path
                                .rsplit('\\')
                                .next()
                                .unwrap_or(&self.exe_path)
                                .to_owned(),
                            process_done: false,
                            thread_at: 0,
                            flags: (args[0] >> 31) as u32,
                        },
                    );
                    self.next_snapshot += 1;
                    handle
                }
            }
            "Process32First" | "Process32Next" | "Process32FirstW" | "Process32NextW"
            | "Thread32First" | "Thread32Next" => {
                let first = name.contains("First");
                let thread = name.starts_with("Thread");
                let size = if thread {
                    28
                } else if name.ends_with('W') {
                    568
                } else {
                    304
                };
                if let Some(snapshot) = self.snapshots.get(&args[0]) {
                    if mem.read_int(args[1], 4)? != size {
                        self.set_error(mem, 24)?;
                        0
                    } else {
                        let index = if first { 0 } else { snapshot.thread_at };
                        let data = if thread {
                            snapshot.thread_entry(index).ok_or(18)
                        } else if !snapshot.process || !first && snapshot.process_done {
                            Err(18)
                        } else {
                            snapshot.process_entry(name.ends_with('W'))
                        };
                        match data {
                            Err(error) => {
                                self.set_error(mem, error)?;
                                0
                            }
                            Ok(data) => {
                                mem.write(args[1], &data)?;
                                let snapshot = self.snapshots.get_mut(&args[0]).unwrap();
                                if thread {
                                    snapshot.thread_at = index + 1;
                                } else {
                                    snapshot.process_done = true;
                                }
                                1
                            }
                        }
                    }
                } else {
                    self.set_error(mem, 6)?;
                    0
                }
            }
            "SystemParametersInfoA" | "SystemParametersInfoW" => {
                // A process-owned logical desktop setting, never a mutation of host preferences.
                match args[0] {
                    0x46 => {
                        mem.write_int(args[2], 4, self.screen_reader as u64)?;
                        1
                    }
                    0x47 if args[3] == 0 => {
                        self.screen_reader = args[1] != 0;
                        1
                    }
                    _ => {
                        self.set_error(mem, 50)?;
                        0
                    }
                }
            }
            "GetSystemMetrics" => match args[0] as u32 {
                0 | 16 | 61 | 78 => 1024,
                1 | 17 | 62 | 79 => 768,
                2 | 3 | 9 | 10 | 20 | 21 => 17,
                4 => 23,
                5..=8 => 1,
                11..=14 => 32,
                15 => 19,
                28..=31 => 112,
                32 | 33 => 4,
                36 | 37 | 48 | 49 => 16,
                43 => 3,
                45 | 46 | 68 | 80 => 1,
                83 => 2,
                _ => 0,
            },
            "CreateThread" => self.create_thread(mem, &args)?,
            "GetThreadErrorMode" => self.threads.entries[&self.threads.active].error_mode as u64,
            "SetThreadErrorMode" => {
                if args[0] & !0x8007 != 0 {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    if args[1] != 0 {
                        mem.check(args[1], 4, WRITE)?;
                    }
                    let old = self.threads.entries[&self.threads.active].error_mode;
                    if args[1] != 0 {
                        mem.write_int(args[1], 4, old as u64)?;
                    }
                    self.threads
                        .entries
                        .get_mut(&self.threads.active)
                        .unwrap()
                        .error_mode = args[0] as u32;
                    1
                }
            }
            "QueueUserAPC" => self.queue_apc(mem, &args)?,
            "SleepEx" => {
                if args[1] != 0 {
                    if let Some(control) =
                        self.deliver_apcs(mem, dll, name, &args, &returned, cpu)?
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
            "ExitThread" => {
                if !self.threads.enabled {
                    return Err("Win64 ExitThread requires a thread scheduler".into());
                }
                self.api_calls += 1;
                self.record(dll, name, &args, args[0]);
                return Ok(Control::ThreadExited(args[0] as u32));
            }
            "Sleep" | "SwitchToThread" => {
                if name == "Sleep" && args[0] != 0 {
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
                if name == "SwitchToThread" {
                    return Ok(Control::SwitchThread(dll.into()));
                }
                self.record(dll, name, &args, 0);
                return Ok(Control::Yielded);
            }
            "ResumeThread"
            | "SuspendThread"
            | "GetThreadId"
            | "GetThreadPriority"
            | "SetThreadPriority"
            | "GetExitCodeThread"
            | "SetThreadAffinityMask" => match self.threads.id(args[0]) {
                Err(error) => {
                    self.set_error(mem, error)?;
                    if matches!(name, "ResumeThread" | "SuspendThread") {
                        u32::MAX as u64
                    } else if name == "GetThreadPriority" {
                        0x7fffffff
                    } else {
                        0
                    }
                }
                Ok(id) => {
                    let t = self.threads.entries.get_mut(&id).unwrap();
                    match name {
                        "GetThreadId" => id,
                        "GetThreadPriority" => t.priority as u32 as u64,
                        "SetThreadPriority" => {
                            let priority = args[1] as i32;
                            if !matches!(priority, -15 | -2..=2 | 15) {
                                self.set_error(mem, 87)?;
                                0
                            } else {
                                t.priority = priority;
                                1
                            }
                        }
                        "GetExitCodeThread" => {
                            mem.write_int(args[1], 4, t.exit.unwrap_or(259) as u64)?;
                            1
                        }
                        "SetThreadAffinityMask" => {
                            if args[1] != 1 {
                                self.set_error(mem, 87)?;
                                0
                            } else {
                                1
                            }
                        }
                        _ => {
                            let old = t.suspended;
                            if name == "SuspendThread" {
                                if old >= 127 {
                                    self.set_error(mem, 87)?;
                                    u32::MAX as u64
                                } else {
                                    t.suspended += 1;
                                    old as u64
                                }
                            } else {
                                t.suspended = old.saturating_sub(1);
                                old as u64
                            }
                        }
                    }
                }
            },
            "DisableThreadLibraryCalls" => {
                // Static TLS needs thread notifications; support explicit failure rather than
                // silently suppressing required callbacks.
                if !self.guest_exports.contains_key(&args[0]) {
                    self.set_error(mem, 6)?;
                } else {
                    self.set_error(mem, 50)?;
                }
                0
            }
            "GetThreadContext" => match self.threads.id(args[0]) {
                Err(error) => {
                    self.set_error(mem, error)?;
                    0
                }
                Ok(id) => {
                    let thread = &self.threads.entries[&id];
                    if id == self.threads.active || thread.suspended == 0 || thread.exit.is_some() {
                        self.set_error(mem, 50)?;
                        0
                    } else if !args[1].is_multiple_of(16) {
                        self.set_error(mem, 998)?;
                        0
                    } else {
                        mem.check(args[1], exception::CONTEXT_SIZE, WRITE)?;
                        let flags = mem.read_int(args[1] + 48, 4)? as u32;
                        self.trace_note(format!("GetThreadContext thread={id} flags={flags:#x}"));
                        if flags & 0x100000 == 0 || flags & !0x10001f != 0 {
                            self.set_error(mem, 87)?;
                            0
                        } else {
                            returned.regs[0] = 1;
                            *cpu = returned;
                            self.api_calls += 1;
                            return Ok(Control::ThreadContext {
                                dll: dll.into(),
                                args,
                                thread: id,
                                flags,
                            });
                        }
                    }
                }
            },
            "SetThreadDescription" | "GetThreadDescription" => match self.threads.id(args[0]) {
                Err(error) => 0x80070000 | error as u64,
                Ok(id) if name == "SetThreadDescription" => {
                    let description = Self::utf16(mem, args[1])?;
                    self.threads.entries.get_mut(&id).unwrap().description = description;
                    0
                }
                Ok(id) => {
                    mem.check(args[1], 8, WRITE)?;
                    let data: Vec<_> = self.threads.entries[&id]
                        .description
                        .encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect();
                    let pointer = self.heap_alloc(mem, data.len() as u64)?;
                    if pointer == 0 {
                        0x8007000e
                    } else {
                        mem.write(pointer, &data)?;
                        mem.write_int(args[1], 8, pointer)?;
                        self.local_blocks.insert(pointer);
                        0
                    }
                }
            },
            "GetActiveProcessorGroupCount" | "GetMaximumProcessorGroupCount" => 1,
            "GetActiveProcessorCount" | "GetMaximumProcessorCount" => {
                if matches!(args[0] as u16, 0 | 0xffff) {
                    1
                } else {
                    self.set_error(mem, 87)?;
                    0
                }
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
            "GetEnvironmentVariableW"
            | "GetEnvironmentVariableA"
            | "SetEnvironmentVariableW"
            | "SetEnvironmentVariableA" => {
                let wide = name.ends_with('W');
                let variable = Self::path_name(mem, args[0], wide)?;
                if variable.is_empty() || variable.contains('=') {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let key = variable.to_lowercase();
                    if name.starts_with("Set") {
                        if args[1] == 0 {
                            if self.environment.remove(&key).is_some() {
                                1
                            } else {
                                self.set_error(mem, 203)?;
                                0
                            }
                        } else {
                            let value = Self::path_name(mem, args[1], wide)?;
                            self.environment.insert(key, (variable, value));
                            1
                        }
                    } else if let Some((_, value)) = self.environment.get(&key) {
                        let bytes: Vec<u8> = if wide {
                            value
                                .encode_utf16()
                                .chain(Some(0))
                                .flat_map(u16::to_le_bytes)
                                .collect()
                        } else {
                            crate::codepage::encode(&(value.clone() + "\0"), 932)
                                .map_err(|e| format!("environment codepage error {e}"))?
                                .0
                        };
                        let required = bytes.len() / if wide { 2 } else { 1 };
                        if args[2] < required as u64 {
                            required as u64
                        } else {
                            mem.write(args[1], &bytes)?;
                            required as u64 - 1
                        }
                    } else {
                        self.set_error(mem, 203)?;
                        0
                    }
                }
            }
            "GlobalMemoryStatusEx" => {
                if mem.read_int(args[0], 4)? != 64 {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let status = mem.memory_status();
                    mem.write(args[0], &status)?;
                    1
                }
            }
            "CreateFileMappingW" | "CreateFileMappingA" => {
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
            "MapViewOfFile" | "MapViewOfFileEx" => {
                match self.mappings.map(
                    mem,
                    args[0],
                    args[1] as u32,
                    args[2] << 32 | args[3],
                    args[4],
                    if name.ends_with("Ex") { args[5] } else { 0 },
                ) {
                    Ok(base) => base,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "UnmapViewOfFile" => {
                if self.mappings.contains_view(args[0]) {
                    self.mappings.unmap(mem, args[0])?;
                    1
                } else {
                    self.set_error(mem, 487)?;
                    0
                }
            }
            "FlushViewOfFile" => match self.mappings.flush(args[0], args[1]) {
                Ok(()) => 1,
                Err(error) => {
                    self.set_error(mem, error)?;
                    0
                }
            },
            "CreateFileW" | "CreateFileA" => {
                if args[6] != 0 {
                    return Err("Win64 file template handle not implemented".into());
                }
                let path = Self::path_name(mem, args[0], name.ends_with('W'))?;
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
                    .fs
                    .as_ref()
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
            "GetFileInformationByHandle" | "GetFileInformationByHandleEx" | "GetFileTime" => {
                let result = (|| -> Result<Vec<u8>, u32> {
                    let (meta, path) = self.files.metadata(args[0])?;
                    let attributes = files::attributes(
                        &meta,
                        path.canonicalize()
                            .ok()
                            .is_some_and(|p| self.not_indexed.contains(&p)),
                    );
                    let kind = if name == "GetFileInformationByHandleEx" {
                        args[1]
                    } else {
                        u64::MAX
                    };
                    let mut data = match kind {
                        u64::MAX if name == "GetFileTime" => vec![0; 24],
                        u64::MAX => vec![0; 52],
                        0 => vec![0; 40],  // FILE_BASIC_INFO
                        1 => vec![0; 24],  // FILE_STANDARD_INFO
                        9 => vec![0; 8], // FILE_ATTRIBUTE_TAG_INFO: host regular files have no reparse tag.
                        18 => vec![0; 24], // FILE_ID_INFO: real host volume and inode identity.
                        _ => return Err(50),
                    };
                    if name == "GetFileInformationByHandleEx" && args[3] < data.len() as u64 {
                        return Err(24);
                    }
                    if kind == u64::MAX || kind == 0 {
                        let offset = if name == "GetFileInformationByHandle" {
                            4
                        } else {
                            0
                        };
                        for (n, time) in [meta.created(), meta.accessed(), meta.modified()]
                            .into_iter()
                            .enumerate()
                        {
                            // GetFileTime permits independent null outputs.
                            if name != "GetFileTime" || args[n + 1] != 0 {
                                data[offset + n * 8..offset + n * 8 + 8]
                                    .copy_from_slice(&files::filetime(time)?.to_le_bytes());
                            }
                        }
                    }
                    match kind {
                        u64::MAX if name == "GetFileTime" => {}
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
                    Ok(data) if name == "GetFileTime" => {
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
                            args[if name == "GetFileInformationByHandle" {
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
            "GetFileSizeEx" | "GetFileSize" | "FlushFileBuffers" => {
                let result = if name == "FlushFileBuffers" {
                    self.files.flush(args[0])
                } else {
                    self.files.size(args[0])
                };
                match result {
                    Ok(value) if name == "GetFileSizeEx" => {
                        mem.write_int(args[1], 8, value)?;
                        1
                    }
                    Ok(value) if name == "GetFileSize" => {
                        if args[1] != 0 {
                            mem.write_int(args[1], 4, value >> 32)?;
                        }
                        self.set_error(mem, 0)?;
                        value as u32 as u64
                    }
                    Ok(value) => value,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        if name == "GetFileSize" {
                            u32::MAX as u64
                        } else {
                            0
                        }
                    }
                }
            }
            "SetFilePointerEx" => {
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
            "GetFileAttributesW"
            | "GetFileAttributesA"
            | "GetFileAttributesExW"
            | "GetFileAttributesExA" => {
                let extended = name.contains("Ex");
                if extended && args[1] != 0 {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let path = Self::path_name(mem, args[0], name.ends_with('W'))?;
                    let fs = self
                        .fs
                        .as_ref()
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
                                    .is_some_and(|p| self.not_indexed.contains(&p))
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
            "SetCurrentDirectoryW" | "SetCurrentDirectoryA" => {
                let path = Self::path_name(mem, args[0], name.ends_with('W'))?;
                match self
                    .fs
                    .as_mut()
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
            "GetCurrentDirectoryW" | "GetCurrentDirectoryA" => {
                let path = self
                    .fs
                    .as_ref()
                    .ok_or("Win64 has no mounted filesystem")?
                    .cwd();
                let wide = name.ends_with('W');
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
            "LocalFree" | "GlobalFree" => {
                if args[0] == 0 {
                    0
                } else if self.local_blocks.contains(&args[0])
                    && self.allocations.contains_key(&args[0])
                {
                    let span = self.allocations[&args[0]].1;
                    mem.unmap(args[0], span)?;
                    self.allocations.remove(&args[0]);
                    self.local_blocks.remove(&args[0]);
                    0
                } else {
                    self.set_error(mem, 6)?;
                    args[0]
                }
            }
            "CoCreateInstance" => {
                if args[0] == 0 || args[3] == 0 || args[4] == 0 {
                    0x80070057
                } else {
                    mem.check(args[4], 8, WRITE)?;
                    let mut clsid = [0; 16];
                    let mut iid = [0; 16];
                    mem.read(args[0], &mut clsid)?;
                    mem.read(args[3], &mut iid)?;
                    self.trace_note(format!("Win64 COM class={clsid:02x?} iid={iid:02x?}: no registered native class factory"));
                    mem.write_int(args[4], 8, 0)?;
                    if self.apartment.is_none() {
                        0x800401f0
                    } else if args[2] == 0 {
                        0x80070057
                    } else {
                        0x80040154
                    }
                }
            }
            "RoInitialize" | "CoInitialize" | "CoInitializeEx" => {
                let winrt = name == "RoInitialize";
                let flags = if name == "CoInitialize" {
                    2
                } else if winrt {
                    0
                } else {
                    args[1]
                };
                let invalid = if winrt {
                    args[0] > 1
                } else {
                    args[0] != 0 || flags & !0xe != 0
                };
                if invalid {
                    0x80070057 // E_INVALIDARG
                } else {
                    let mode = if winrt {
                        if args[0] == 0 {
                            2
                        } else {
                            0
                        }
                    } else {
                        flags as u32 & 2
                    };
                    if self.apartment.is_some_and(|current| current != mode) {
                        0x80010106 // RPC_E_CHANGED_MODE, no new reference.
                    } else {
                        let result = self.apartment.is_some() as u64;
                        self.apartment = Some(mode);
                        let refs = if winrt {
                            &mut self.winrt_refs
                        } else {
                            &mut self.com_refs
                        };
                        *refs = refs
                            .checked_add(1)
                            .ok_or("Win64 apartment reference overflow")?;
                        result
                    }
                }
            }
            "RoUninitialize" | "CoUninitialize" => {
                let refs = if name == "RoUninitialize" {
                    &mut self.winrt_refs
                } else {
                    &mut self.com_refs
                };
                *refs = refs.saturating_sub(1);
                if self.com_refs == 0 && self.winrt_refs == 0 {
                    self.apartment = None;
                }
                0
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
                    self.record(dll, name, &args, 0);
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
                    .fs
                    .as_ref()
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
            "GetHandleInformation" => {
                if let Some(flags) = self
                    .pipes
                    .flags(args[0])
                    .or_else(|| self.events.flags(args[0]))
                    .or_else(|| self.files.flags(args[0]))
                    .or_else(|| self.mappings.flags(args[0]))
                    .or_else(|| self.threads.flags(args[0]))
                    .or_else(|| self.snapshots.get(&args[0]).map(|s| s.flags))
                    .or_else(|| self.std_flags(args[0]))
                    .or_else(|| self.tokens.get(&args[0]).map(|h| h.1))
                {
                    mem.write_int(args[1], 4, flags as u64)?;
                    1
                } else {
                    self.set_error(mem, 6)?;
                    0
                }
            }
            "SetHandleInformation" => {
                if let Some((_, flags)) = self.tokens.get_mut(&args[0]) {
                    let result = if args[1] & !3 != 0 {
                        0
                    } else {
                        *flags = *flags & !(args[1] as u32) | args[2] as u32 & args[1] as u32;
                        1
                    };
                    if result == 0 {
                        self.set_error(mem, 87)?;
                    }
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, &args, result);
                    return Ok(Control::Returned);
                }
                if let Some(old) = self.std_flags(args[0]) {
                    let result = if args[1] & !3 != 0 {
                        self.set_error(mem, 87)?;
                        0
                    } else {
                        let new = old & !(args[1] as u32) | args[2] as u32 & args[1] as u32;
                        if let Some(alias) = self.std_aliases.get_mut(&args[0]) {
                            alias.1 = new;
                        } else {
                            self.std_flags[(args[0] - 0x100) as usize] = new;
                        }
                        1
                    };
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, &args, result);
                    return Ok(Control::Returned);
                }
                if let Some(s) = self.snapshots.get_mut(&args[0]) {
                    let result = if args[1] & !3 != 0 {
                        0
                    } else {
                        s.flags = (s.flags & !(args[1] as u32)) | (args[2] as u32 & args[1] as u32);
                        1
                    };
                    if result == 0 {
                        self.set_error(mem, 87)?;
                    }
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, &args, result);
                    return Ok(Control::Returned);
                }
                if let Some(flags) = self.threads.handle_flags.get_mut(&args[0]) {
                    if args[1] & !3 != 0 {
                        self.set_error(mem, 87)?;
                        returned.regs[0] = 0;
                        *cpu = returned;
                        self.api_calls += 1;
                        self.record(dll, name, &args, 0);
                        return Ok(Control::Returned);
                    }
                    *flags = *flags & !(args[1] as u32) | args[2] as u32 & args[1] as u32;
                    returned.regs[0] = 1;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, &args, 1);
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
            "ReadFile" => {
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
                    self.record(dll, name, &args, result);
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
            name if timers::arity(name).is_some() => self.timer_call(mem, name, &args)?,
            "CreateEventW" | "CreateEventA" | "CreateEventExW" | "CreateEventExA"
            | "OpenEventW" | "OpenEventA" => {
                let extended = name.contains("Ex");
                let opening = name.starts_with("Open");
                if !opening && args[0] != 0 {
                    return Err("Win64 event SECURITY_ATTRIBUTES not implemented".into());
                }
                let pointer = args[if opening {
                    2
                } else if extended {
                    1
                } else {
                    3
                }];
                let event_name = if pointer == 0 {
                    None
                } else if name.ends_with('W') {
                    Some(Self::utf16(mem, pointer)?)
                } else {
                    let bytes = Self::string_bytes(mem, pointer, u32::MAX as u64, 1)?;
                    Some(
                        crate::codepage::decode(&bytes[..bytes.len() - 1], 932)
                            .map_err(|e| format!("event codepage error {e}"))?,
                    )
                };
                let result = if opening {
                    if let Some(event_name) = event_name {
                        self.events
                            .open(event_name, args[0] as u32, false)
                            .map(|h| (h, false))
                    } else {
                        Err(87)
                    }
                } else if extended && args[2] & !3 != 0 {
                    Err(87)
                } else {
                    let (manual, initial, access) = if extended {
                        (args[2] & 1 != 0, args[2] & 2 != 0, args[3] as u32)
                    } else {
                        (args[1] != 0, args[2] != 0, 0x1f0003)
                    };
                    self.events.create(manual, initial, event_name, access)
                };
                match result {
                    Ok((handle, existed)) => {
                        self.set_error(mem, if existed { 183 } else { 0 })?;
                        handle
                    }
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "CreateSemaphoreW" | "CreateSemaphoreA" | "CreateSemaphoreExW"
            | "CreateSemaphoreExA" | "OpenSemaphoreW" | "OpenSemaphoreA" => {
                let opening = name.starts_with("Open");
                if !opening && args[0] != 0 {
                    return Err("Win64 semaphore SECURITY_ATTRIBUTES not implemented".into());
                }
                let pointer = args[if opening { 2 } else { 3 }];
                let object_name = if pointer == 0 {
                    None
                } else if name.ends_with('W') {
                    Some(Self::utf16(mem, pointer)?)
                } else {
                    let bytes = Self::string_bytes(mem, pointer, u32::MAX as u64, 1)?;
                    Some(
                        crate::codepage::decode(&bytes[..bytes.len() - 1], 932)
                            .map_err(|e| format!("semaphore codepage error {e}"))?,
                    )
                };
                let result = if opening {
                    object_name.ok_or(87u32).and_then(|n| {
                        self.events
                            .open(n, args[0] as u32, true)
                            .map(|h| (h, false))
                    })
                } else if name.contains("Ex") && args[4] != 0 {
                    Err(87)
                } else {
                    let access = if name.contains("Ex") {
                        args[5] as u32
                    } else {
                        0x1f0003
                    };
                    self.events
                        .semaphore(args[1] as i32, args[2] as i32, object_name, access)
                };
                match result {
                    Ok((handle, existed)) => {
                        self.set_error(mem, if existed { 183 } else { 0 })?;
                        handle
                    }
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "ReleaseSemaphore" => match self.events.release_count(args[0], args[1] as i32) {
                Ok(previous) => {
                    if args[2] != 0 {
                        mem.write_int(args[2], 4, previous as u64)?;
                    }
                    self.events.release(args[0], args[1] as i32);
                    1
                }
                Err(error) => {
                    self.set_error(mem, error)?;
                    0
                }
            },
            "SetEvent" | "ResetEvent" => match self.events.change(args[0], name == "SetEvent") {
                Ok(result) => result,
                Err(error) => {
                    self.set_error(mem, error)?;
                    0
                }
            },
            "WaitForSingleObject"
            | "WaitForSingleObjectEx"
            | "WaitForMultipleObjects"
            | "WaitForMultipleObjectsEx" => {
                let multiple = name.starts_with("WaitForMultiple");
                let (handles, all, timeout) = if multiple {
                    if args[0] == 0 || args[0] > 64 {
                        self.set_error(mem, 87)?;
                        returned.regs[0] = u32::MAX as u64;
                        *cpu = returned;
                        self.api_calls += 1;
                        self.record(dll, name, &args, u32::MAX as u64);
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
                let alertable = name.ends_with("Ex") && args[if multiple { 4 } else { 2 }] != 0;
                if alertable && self.wait_objects(&handles, all, false).is_ok() {
                    if let Some(control) =
                        self.deliver_apcs(mem, dll, name, &args, &returned, cpu)?
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
            "VirtualAlloc" | "VirtualAllocEx" | "VirtualFree" | "VirtualFreeEx"
            | "VirtualProtect" | "VirtualProtectEx" | "VirtualQuery" | "VirtualQueryEx" => {
                let extended = name.ends_with("Ex");
                let virtual_name = name.strip_suffix("Ex").unwrap_or(name);
                let offset = extended as usize;
                let result: Result<u64, u32> = if extended && args[0] != u64::MAX {
                    Err(6)
                } else {
                    let a = &args[offset..];
                    match virtual_name {
                        "VirtualAlloc" => mem.virtual_alloc(a[0], a[1], a[2] as u32, a[3] as u32),
                        "VirtualFree" => mem.virtual_free(a[0], a[1], a[2] as u32).map(|()| 1),
                        "VirtualProtect" => {
                            mem.check(a[3], 4, WRITE)?;
                            match mem.validate_protect(a[0], a[1], a[2] as u32) {
                                Ok(old) => {
                                    // Preflight every page and copy out before applying the
                                    // change, including when the output aliases the target.
                                    mem.write_int(a[3], 4, old as u64)?;
                                    mem.virtual_protect(a[0], a[1], a[2] as u32).map(|_| 1)
                                }
                                Err(error) => Err(error),
                            }
                        }
                        "VirtualQuery" => {
                            if a[2] < 48 {
                                Err(24)
                            } else {
                                match mem.virtual_query(a[0]) {
                                    Ok(info) => {
                                        mem.write(a[1], &info)?;
                                        Ok(48)
                                    }
                                    Err(error) => Err(error),
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                };
                match result {
                    Ok(value) => value,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "GetSystemTime" | "GetLocalTime" => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?;
                let instant: libc::time_t = now
                    .as_secs()
                    .try_into()
                    .map_err(|_| "host time out of range")?;
                let mut calendar: libc::tm = unsafe { std::mem::zeroed() };
                unsafe {
                    let result = if name == "GetLocalTime" {
                        libc::localtime_r(&instant, &mut calendar)
                    } else {
                        libc::gmtime_r(&instant, &mut calendar)
                    };
                    if result.is_null() {
                        return Err("host calendar conversion failed".into());
                    }
                }
                let fields = [
                    calendar.tm_year + 1900,
                    calendar.tm_mon + 1,
                    calendar.tm_wday,
                    calendar.tm_mday,
                    calendar.tm_hour,
                    calendar.tm_min,
                    calendar.tm_sec,
                    now.subsec_millis() as i32,
                ];
                let mut output = [0; 16];
                for (n, value) in fields.into_iter().enumerate() {
                    let value =
                        u16::try_from(value).map_err(|_| "Win64 calendar value out of range")?;
                    output[n * 2..n * 2 + 2].copy_from_slice(&value.to_le_bytes());
                }
                mem.write(args[0], &output)?;
                0
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
            "GetTimeZoneInformation" => match Self::host_zone() {
                Ok((mode, data)) => {
                    mem.write(args[0], &data)?;
                    mode
                }
                Err(error) => {
                    self.set_error(mem, error)?;
                    u32::MAX as u64
                }
            },
            "SystemTimeToTzSpecificLocalTime" | "TzSpecificLocalTimeToSystemTime" => {
                match self.convert_zone(mem, &args, name == "TzSpecificLocalTimeToSystemTime")? {
                    Ok(output) => {
                        mem.write(args[2], &output)?;
                        1
                    }
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            "SystemTimeToFileTime" => {
                let mut bytes = [0; 16];
                mem.read(args[0], &mut bytes)?;
                let fields =
                    std::array::from_fn(|n| u16::from_le_bytes([bytes[n * 2], bytes[n * 2 + 1]]));
                if let Some(ticks) = Self::calendar_to_ticks(fields) {
                    mem.write_int(args[1], 8, ticks)?;
                    1
                } else {
                    self.set_error(mem, 87)?;
                    0
                }
            }
            "FileTimeToSystemTime" => {
                let ticks = mem.read_int(args[0], 8)?;
                if let Some(fields) = Self::ticks_to_calendar(ticks) {
                    let bytes: Vec<u8> = fields.into_iter().flat_map(u16::to_le_bytes).collect();
                    mem.write(args[1], &bytes)?;
                    1
                } else {
                    self.set_error(mem, 87)?;
                    0
                }
            }
            "CompareFileTime" => {
                use std::cmp::Ordering;
                match mem.read_int(args[0], 8)?.cmp(&mem.read_int(args[1], 8)?) {
                    Ordering::Less => u32::MAX as u64,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                }
            }
            "GetSystemInfo" | "GetNativeSystemInfo" => {
                let mut info = [0; 48];
                info[..2].copy_from_slice(&9u16.to_le_bytes()); // PROCESSOR_ARCHITECTURE_AMD64
                info[4..8].copy_from_slice(&(PAGE as u32).to_le_bytes());
                info[8..16].copy_from_slice(&0x10000u64.to_le_bytes());
                info[16..24].copy_from_slice(&((1u64 << 47) - 65537).to_le_bytes());
                info[24..32].copy_from_slice(&1u64.to_le_bytes()); // One virtual guest CPU.
                info[32..36].copy_from_slice(&1u32.to_le_bytes());
                info[36..40].copy_from_slice(&8664u32.to_le_bytes()); // PROCESSOR_AMD_X8664
                info[40..44].copy_from_slice(&65536u32.to_le_bytes());
                info[44..46].copy_from_slice(&6u16.to_le_bytes());
                mem.write(args[0], &info)?;
                0
            }
            "GetSystemTimeAsFileTime" | "GetSystemTimePreciseAsFileTime" => {
                let elapsed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?;
                let time = 116444736000000000u64 + (elapsed.as_nanos() / 100) as u64;
                mem.write_int(args[0], 8, time)?;
                0
            }
            "QueryPerformanceCounter" => {
                mem.write_int(args[0], 8, (self.start.elapsed().as_nanos() / 100) as u64)?;
                1
            }
            "QueryPerformanceFrequency" => {
                mem.write_int(args[0], 8, 10_000_000)?;
                1
            }
            "GetCurrentProcessId" => PID,
            "GetCurrentThreadId" => self.threads.active,
            "GetCurrentProcess" => u64::MAX,
            "GetCurrentThread" => u64::MAX - 1,
            "IsDebuggerPresent" => mem.read_int(PEB + 2, 1)?,
            "GetTickCount" | "timeGetTime" => self.start.elapsed().as_millis() as u32 as u64,
            "timeGetDevCaps" => {
                if args[1] < 8 || args[0] == 0 {
                    97
                } else {
                    mem.write(
                        args[0],
                        &[1u32.to_le_bytes(), u32::MAX.to_le_bytes()].concat(),
                    )?;
                    0
                }
            }
            "timeBeginPeriod" => {
                let period = args[0] as u32;
                if period == 0
                    || (self.timer_periods.len() >= 256
                        && !self.timer_periods.contains_key(&period))
                {
                    97
                } else {
                    let refs = self.timer_periods.entry(period).or_default();
                    *refs = refs
                        .checked_add(1)
                        .ok_or("multimedia timer reference capacity")?;
                    0
                }
            }
            "timeEndPeriod" => {
                let period = args[0] as u32;
                if let Some(refs) = self.timer_periods.get_mut(&period) {
                    *refs -= 1;
                    if *refs == 0 {
                        self.timer_periods.remove(&period);
                    }
                    0
                } else {
                    97
                }
            }
            "GetTickCount64" => self.start.elapsed().as_millis() as u64,
            "GetLastError" => mem.read_int(self.teb() + 0x68, 4)?,
            "SetLastError" => {
                self.set_error(mem, args[0] as u32)?;
                0
            }
            "GetCommandLineA" => COMMAND_A,
            "GetCommandLineW" => COMMAND_W,
            "EncodePointer" | "DecodePointer" => args[0], // Optional pointer obfuscation; inverse identity.
            "GetProcessHeap" => PROCESS_HEAP,
            "InitializeConditionVariable"
            | "WakeConditionVariable"
            | "WakeAllConditionVariable" => {
                if name == "InitializeConditionVariable" {
                    mem.write_int(args[0], 8, 0)?;
                } else if mem.read_int(args[0], 8)? != 0 {
                    return Err(
                        "Win64 condition-variable wait list requires a thread scheduler".into(),
                    );
                }
                // There are no waiting guest threads in this single-thread provider.
                0
            }
            "InitializeSRWLock"
            | "AcquireSRWLockExclusive"
            | "ReleaseSRWLockExclusive"
            | "AcquireSRWLockShared"
            | "ReleaseSRWLockShared"
            | "TryAcquireSRWLockExclusive"
            | "TryAcquireSRWLockShared" => {
                let pointer = args[0];
                mem.check(pointer, 8, WRITE)?;
                let token = mem.read_int(pointer, 8)?;
                let (owner, readers) = self.srw.get(&pointer).copied().unwrap_or((0, 0));
                let expected = if owner != 0 {
                    1
                } else if readers != 0 {
                    (readers as u64) * 4 + 2
                } else {
                    0
                };
                if name != "InitializeSRWLock" && token != expected {
                    return Err("invalid Win64 SRW lock state".into());
                }
                let shared = name.ends_with("Shared");
                let release = name.starts_with("Release");
                let attempt = name.starts_with("Try");
                let state = if name == "InitializeSRWLock" {
                    (0, 0)
                } else if release {
                    if shared {
                        if readers == 0 || owner != 0 {
                            return Err("unowned Win64 shared SRW lock".into());
                        }
                        (0, readers - 1)
                    } else {
                        if owner != self.threads.active || readers != 0 {
                            return Err("unowned Win64 exclusive SRW lock".into());
                        }
                        (0, 0)
                    }
                } else if owner != 0 || (!shared && readers != 0) {
                    if !attempt {
                        self.block(threads::Block::Srw(pointer, shared))?;
                        return Ok(Control::Blocked);
                    }
                    returned.regs[0] = 0;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, &args, 0);
                    return Ok(Control::Returned);
                } else if shared {
                    (0, readers.checked_add(1).ok_or("SRW reader overflow")?)
                } else {
                    (self.threads.active, 0)
                };
                let token = if state.0 != 0 {
                    1
                } else if state.1 != 0 {
                    state.1 as u64 * 4 + 2
                } else {
                    0
                };
                mem.write_int(pointer, 8, token)?;
                if token == 0 {
                    self.srw.remove(&pointer);
                } else {
                    self.srw.insert(pointer, state);
                }
                self.unblock();
                attempt as u64
            }
            "InitOnceBeginInitialize" => {
                if args[1] & !1 != 0 {
                    return Err("Win64 asynchronous InitOnce not implemented".into());
                }
                let state = mem.read_int(args[0], 8)?;
                mem.check(args[2], 4, WRITE)?;
                if args[3] != 0 {
                    mem.check(args[3], 8, WRITE)?;
                }
                if state & 3 == 2 {
                    mem.write_int(args[2], 4, 0)?;
                    if args[3] != 0 {
                        mem.write_int(args[3], 8, state & !3)?;
                    }
                    1
                } else if args[1] & 1 != 0 {
                    self.set_error(mem, 31)?;
                    0
                } else if state == 0 {
                    mem.check(args[0], 8, WRITE)?;
                    mem.write_int(args[0], 8, 1)?;
                    mem.write_int(args[2], 4, 1)?;
                    if args[3] != 0 {
                        mem.write_int(args[3], 8, 0)?;
                    }
                    1
                } else {
                    self.block(threads::Block::Once(args[0]))?;
                    return Ok(Control::Blocked);
                }
            }
            "InitOnceComplete" => {
                let failed = args[1] & 4 != 0;
                if args[1] & !4 != 0
                    || args[2] & 3 != 0
                    || (failed && args[2] != 0)
                    || mem.read_int(args[0], 8)? != 1
                {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    mem.write_int(args[0], 8, if failed { 0 } else { args[2] | 2 })?;
                    1
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
            "GetStringTypeW" => {
                if args[0] as u32 != 1 {
                    self.set_error(mem, 1004)?;
                    0
                } else {
                    let bytes = Self::string_bytes(mem, args[1], args[2], 2)?;
                    let output: Vec<u8> = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .flat_map(|b| {
                            crate::nls::ctype1(u16::from_le_bytes([b[0], b[1]])).to_le_bytes()
                        })
                        .collect();
                    mem.write(args[3], &output)?;
                    1
                }
            }
            "GetEnvironmentStringsW" => {
                // Initially empty; changes are guest-owned and never inherit host secrets.
                let mut bytes: Vec<u8> = self
                    .environment
                    .values()
                    .flat_map(|(name, value)| {
                        format!("{name}={value}\0")
                            .encode_utf16()
                            .flat_map(u16::to_le_bytes)
                            .collect::<Vec<_>>()
                    })
                    .collect();
                bytes.extend_from_slice(&[0, 0]);
                if bytes.len() == 2 {
                    bytes.extend_from_slice(&[0, 0]);
                }
                let ptr = self.heap_alloc(mem, bytes.len() as u64)?;
                if ptr != 0 {
                    mem.write(ptr, &bytes)?;
                    self.environment_blocks.insert(ptr);
                }
                ptr
            }
            "FreeEnvironmentStringsW" => {
                if !self.environment_blocks.contains(&args[0])
                    || !self.allocations.contains_key(&args[0])
                {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let (_, span) = self.allocations[&args[0]];
                    mem.unmap(args[0], span)?;
                    self.environment_blocks.remove(&args[0]);
                    self.allocations.remove(&args[0]);
                    1
                }
            }
            "GetModuleFileNameW" | "GetModuleFileNameA" => {
                let path = if args[0] == 0 || args[0] == self.image_base {
                    &self.exe_path
                } else {
                    self.guest_paths
                        .get(&args[0])
                        .ok_or("Win64 native module filename query not implemented")?
                };
                let wide = name.ends_with('W');
                let bytes = if wide {
                    path.encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&(path.clone() + "\0"), 932)
                        .map_err(|e| format!("codepage error {e}"))?
                        .0
                };
                let width = if wide { 2 } else { 1 };
                let capacity = args[2] as u32 as usize;
                if capacity == 0 {
                    self.set_error(mem, 122)?;
                    0
                } else {
                    let units = bytes.len() / width;
                    let n = capacity.min(units);
                    let mut output = bytes[..n * width].to_vec();
                    if n < units {
                        output[(n - 1) * width..].fill(0);
                    }
                    mem.write(args[1], &output)?;
                    if n < units {
                        self.set_error(mem, 122)?;
                        n as u64
                    } else {
                        (units - 1) as u64
                    }
                }
            }
            "MultiByteToWideChar" | "WideCharToMultiByte" | "LCMapStringW" | "LCMapStringEx" => {
                if name == "LCMapStringEx" {
                    let locale = if args[0] == 0 {
                        "ja-JP".into()
                    } else {
                        Self::utf16(mem, args[0])?
                    };
                    if !matches!(locale.as_str(), "" | "ja-JP" | "en-US")
                        || args[6..].iter().any(|&a| a != 0)
                    {
                        return Err("Win64 locale/version/sort handle not implemented".into());
                    }
                }
                let wide_input = name != "MultiByteToWideChar";
                let input =
                    Self::string_bytes(mem, args[2], args[3], if wide_input { 2 } else { 1 })?;
                let output: Result<(Vec<u8>, bool), u32> = if name == "MultiByteToWideChar" {
                    if args[1] as u32 & !9 != 0 {
                        Err(1004)
                    } else {
                        let text = if args[1] & 8 != 0 {
                            crate::codepage::decode(&input, args[0] as u32)
                        } else {
                            crate::codepage::decode_lossy(&input, args[0] as u32)
                        };
                        text.map(|text| {
                            (
                                text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                                false,
                            )
                        })
                    }
                } else {
                    let units: Vec<u16> = input
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect();
                    if name == "LCMapStringW" || name == "LCMapStringEx" {
                        let upper = args[1] & 0x200 != 0;
                        let lower = args[1] & 0x100 != 0;
                        if upper == lower || args[1] & !0x01000300 != 0 {
                            Err(1004)
                        } else {
                            String::from_utf16(&units).map_err(|_| 1113).map(|text| {
                                let mapped = if upper {
                                    text.to_uppercase()
                                } else {
                                    text.to_lowercase()
                                };
                                (
                                    mapped.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                                    false,
                                )
                            })
                        }
                    } else if args[6] != 0 {
                        return Err(
                            "Win64 custom codepage default character is not implemented".into()
                        );
                    } else if args[1] & !0x480 != 0
                        || (args[0] as u32 == 65001 && (args[7] != 0 || args[1] & !0x80 != 0))
                    {
                        Err(1004)
                    } else {
                        let text = if args[1] & 0x80 != 0 {
                            String::from_utf16(&units).map_err(|_| 1113)
                        } else {
                            Ok(String::from_utf16_lossy(&units))
                        };
                        text.and_then(|text| crate::codepage::encode(&text, args[0] as u32))
                    }
                };
                match output {
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                    Ok((output, used)) => {
                        let count =
                            output.len() / if name == "WideCharToMultiByte" { 1 } else { 2 };
                        let capacity = args[5] as u32 as i32;
                        if capacity < 0 || (capacity != 0 && (capacity as usize) < count) {
                            self.set_error(mem, 122)?;
                            0
                        } else {
                            if capacity != 0 {
                                mem.check(args[4], output.len(), WRITE)?;
                            }
                            if name == "WideCharToMultiByte" && args[7] != 0 {
                                mem.check(args[7], 4, WRITE)?;
                            }
                            if capacity != 0 {
                                mem.write(args[4], &output)?;
                            }
                            if name == "WideCharToMultiByte" && args[7] != 0 {
                                mem.write_int(args[7], 4, used as u64)?;
                            }
                            count as u64
                        }
                    }
                }
            }
            "GetACP" | "GetOEMCP" | "GetConsoleOutputCP" | "GetConsoleCP" => 932,
            "IsValidCodePage" => matches!(args[0] as u32, 932 | 1252 | 65001) as u64,
            "GetCPInfo" | "GetCPInfoExA" | "GetCPInfoExW" => {
                let extended = name != "GetCPInfo";
                let cp = match args[0] as u32 {
                    0 | 1 | 3 => 932,
                    value => value,
                };
                if extended && args[1] != 0 {
                    self.set_error(mem, 1004)?;
                    0
                } else if !matches!(cp, 932 | 1252 | 65001) {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let wide = name.ends_with('W');
                    let mut info = vec![
                        0;
                        if !extended {
                            20
                        } else if wide {
                            544
                        } else {
                            284
                        }
                    ];
                    info[..4].copy_from_slice(
                        &(if cp == 932 {
                            2u32
                        } else if cp == 65001 {
                            4
                        } else {
                            1
                        })
                        .to_le_bytes(),
                    );
                    info[4] = b'?';
                    if cp == 932 {
                        info[6..10].copy_from_slice(&[0x81, 0x9f, 0xe0, 0xfc]);
                    }
                    if extended {
                        let unicode_default = if cp == 932 {
                            0x30fbu16
                        } else if cp == 65001 {
                            0xfffd
                        } else {
                            0x3f
                        };
                        info[18..20].copy_from_slice(&unicode_default.to_le_bytes());
                        info[20..24].copy_from_slice(&cp.to_le_bytes());
                        let name = match cp {
                            932 => "932 (Japanese Shift-JIS)",
                            1252 => "1252 (Western European)",
                            _ => "65001 (UTF-8)",
                        };
                        let bytes: Vec<u8> = if wide {
                            name.encode_utf16().flat_map(u16::to_le_bytes).collect()
                        } else {
                            name.as_bytes().to_vec()
                        };
                        info[24..24 + bytes.len()].copy_from_slice(&bytes);
                    }
                    mem.write(args[if extended { 2 } else { 1 }], &info)?;
                    1
                }
            }
            "GetStdHandle" | "SetStdHandle" => {
                let index = match args[0] as u32 {
                    0xfffffff6 => Some(0),
                    0xfffffff5 => Some(1),
                    0xfffffff4 => Some(2),
                    _ => None,
                };
                if let Some(index) = index {
                    if name == "GetStdHandle" {
                        self.std_handles[index]
                    } else {
                        self.std_handles[index] = args[1];
                        1
                    }
                } else {
                    self.set_error(mem, 87)?;
                    if name == "GetStdHandle" {
                        u64::MAX
                    } else {
                        0
                    }
                }
            }
            "GetFileType" | "GetConsoleMode" | "CloseHandle" => {
                let index = self.std_index(args[0]);
                if index.is_some() {
                    match name {
                        "GetFileType" => 3, // Redirected host pipe, not a console.
                        "GetConsoleMode" => {
                            self.set_error(mem, 6)?;
                            0
                        }
                        _ => match self.close_std(args[0]) {
                            Ok(()) => 1,
                            Err(error) => {
                                self.set_error(mem, error)?;
                                0
                            }
                        },
                    }
                } else if self.files.flags(args[0]).is_some() {
                    match name {
                        "GetFileType" => 1,
                        "CloseHandle" => match self.files.close(args[0]) {
                            Ok(()) => 1,
                            Err(error) => {
                                self.set_error(mem, error)?;
                                0
                            }
                        },
                        _ => {
                            self.set_error(mem, 6)?;
                            0
                        }
                    }
                } else if self.pipes.flags(args[0]).is_some() {
                    match name {
                        "GetFileType" => 3,
                        "CloseHandle" => match self.pipes.close(args[0]) {
                            Ok(()) => 1,
                            Err(error) => {
                                self.set_error(mem, error)?;
                                0
                            }
                        },
                        _ => {
                            self.set_error(mem, 6)?;
                            0
                        }
                    }
                } else if name == "CloseHandle" && self.mappings.flags(args[0]).is_some() {
                    match self.mappings.close(args[0]) {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    }
                } else if name == "CloseHandle" && self.tokens.contains_key(&args[0]) {
                    if self.tokens[&args[0]].1 & 2 != 0 {
                        self.set_error(mem, 6)?;
                        0
                    } else {
                        self.tokens.remove(&args[0]);
                        1
                    }
                } else if name == "CloseHandle" && self.snapshots.contains_key(&args[0]) {
                    if self.snapshots[&args[0]].flags & 2 != 0 {
                        self.set_error(mem, 5)?;
                        0
                    } else {
                        self.snapshots.remove(&args[0]);
                        1
                    }
                } else if name == "CloseHandle" && self.threads.flags(args[0]).is_some() {
                    match self.threads.close(args[0]) {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    }
                } else if name == "CloseHandle" && self.events.close(args[0]) {
                    1
                } else {
                    self.set_error(mem, 6)?;
                    0
                }
            }
            "WriteFile" | "WriteConsoleW" => {
                if name == "WriteFile" && self.files.flags(args[0]).is_some() {
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
                    self.record(dll, name, &args, result);
                    return Ok(Control::Returned);
                }
                if name == "WriteFile" && self.pipes.flags(args[0]).is_some() {
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
                    self.record(dll, name, &args, value);
                    return Ok(Control::Returned);
                }
                let index = self.std_index(args[0]).filter(|&i| i != 0);
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
            "GetModuleHandleW" | "GetModuleHandleA" => {
                if args[0] == 0 {
                    self.image_base
                } else {
                    let module = if name.ends_with('W') {
                        Self::utf16(mem, args[0])?
                    } else {
                        Self::ascii(mem, args[0])?
                    };
                    let handle = self.native_module(&module, false)?;
                    if handle == 0 {
                        self.set_error(mem, 126)?;
                    }
                    handle
                }
            }
            "LoadLibraryExW" | "LoadLibraryExA" | "LoadLibraryW" | "LoadLibraryA" => {
                let flags = if arity == 3 { args[2] } else { 0 };
                if arity == 3 && (args[1] != 0 || flags & !0x1808 != 0) {
                    return Err("unsupported Win64 LoadLibraryEx flags/file".into());
                }
                let module = Self::path_name(mem, args[0], name.ends_with('W'))?;
                if flags & 8 != 0 && (flags & 0x1800 != 0 || !Self::absolute_module_path(&module)) {
                    self.set_error(mem, 87)?;
                    returned.regs[0] = 0;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, &args, 0);
                    return Ok(Control::Returned);
                }
                match self.native_module(&module, true) {
                    Ok(handle) => handle,
                    Err(error) => {
                        let path = self.fs.as_ref().ok_or(126u32).and_then(|fs| {
                            self.module_path(fs, &module, flags)
                                .map(|path| fs.guest_path(&path))
                        });
                        if let Ok(path) = path {
                            if !self.threads.enabled {
                                return Err(error);
                            }
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
                                args,
                            });
                        }
                        self.set_error(mem, path.unwrap_err())?;
                        0
                    }
                }
            }
            "FreeLibrary" => {
                if self.guest_exports.contains_key(&args[0]) {
                    let refs = &mut self.modules.get_mut(&args[0]).unwrap().1;
                    if *refs <= 1 && !self.pinned_modules.contains(&args[0]) {
                        return Err("Win64 guest DLL detach/unload not implemented".into());
                    }
                    *refs = refs.saturating_sub(1).max(1);
                    returned.regs[0] = 1;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, &args, 1);
                    return Ok(Control::Returned);
                }
                let Some((module, refs)) = self.modules.get_mut(&args[0]) else {
                    return Err("invalid Win64 module handle".into());
                };
                if (module != "kernel32.dll" && !self.pinned_modules.contains(&args[0]))
                    || *refs > 1
                {
                    *refs -= 1;
                    if *refs == 0 {
                        self.modules.remove(&args[0]);
                    }
                }
                1
            }
            "GetProcAddress" => {
                // Windows resolves a NULL module against the main executable.
                let module_handle = if args[0] == 0 {
                    self.image_base
                } else {
                    args[0]
                };
                let symbol = if args[1] <= 65535 {
                    format!("#{}", args[1])
                } else {
                    Self::ascii(mem, args[1])?
                };
                self.trace_note(format!(
                    "GetProcAddress lookup module={:#x} symbol={symbol}",
                    args[0]
                ));
                if let Some(exports) = self.guest_exports.get(&module_handle) {
                    let address = if args[1] <= 65535 {
                        exports.ordinals.get(&(args[1] as u16)).copied()
                    } else {
                        exports.names.get(&Self::ascii(mem, args[1])?).copied()
                    };
                    if address.is_none() {
                        self.set_error(mem, 127)?;
                    }
                    address.unwrap_or(0)
                } else {
                    let Some((module, _)) = self.modules.get(&module_handle) else {
                        self.set_error(
                            mem,
                            if module_handle == self.image_base {
                                127
                            } else {
                                126
                            },
                        )?;
                        returned.regs[0] = 0;
                        *cpu = returned;
                        self.unblock();
                        self.api_calls += 1;
                        self.record(dll, name, &args, 0);
                        return Ok(Control::Returned);
                    };
                    let function = if args[1] <= 65535 {
                        if module.eq_ignore_ascii_case("ws2_32.dll") {
                            sockets::ordinal(&symbol).unwrap_or(&symbol).to_owned()
                        } else if module.eq_ignore_ascii_case("d3d12.dll") {
                            dxgi::symbol(module, &symbol).to_owned()
                        } else {
                            return Err("Win64 native ordinal lookup is not implemented".into());
                        }
                    } else {
                        Self::ascii(mem, args[1])?
                    };
                    let provider = if module.starts_with("api-ms-win-core-") {
                        "kernel32.dll"
                    } else {
                        module.as_str()
                    }
                    .to_owned();
                    let known = (matches!(provider.as_str(), "kernel32.dll" | "kernelbase.dll")
                        && kernel_export(&function))
                        || (matches!(provider.as_str(), "kernel32.dll" | "kernelbase.dll")
                            && timers::arity(&function).is_some())
                        || (matches!(
                            provider.as_str(),
                            "ntdll.dll" | "kernel32.dll" | "kernelbase.dll"
                        ) && unwind::is_call(&function))
                        || (matches!(
                            provider.as_str(),
                            "kernel32.dll" | "kernelbase.dll" | "ntdll.dll"
                        ) && version::is_call(&function))
                        || (provider == "ws2_32.dll" && sockets::is_call(&function))
                        || (provider == "secur32.dll" && sspi::arity(&function).is_some())
                        || (provider == "dxgi.dll" && dxgi::export(&function))
                        || (provider == "d3d12.dll" && dxgi::d3d12_export(&function))
                        || (provider == "d3d11.dll" && dxgi::d3d11_export(&function))
                        || (provider == "winmm.dll"
                            && matches!(
                                function.as_str(),
                                "timeGetTime"
                                    | "timeBeginPeriod"
                                    | "timeEndPeriod"
                                    | "timeGetDevCaps"
                            ))
                        || (provider == "user32.dll" && display::arity(&function).is_some())
                        || (provider == "user32.dll" && user::arity(&function).is_some())
                        || (provider == "imm32.dll" && user::ime_arity(&function).is_some())
                        || (provider == "gdi32.dll" && user::gdi_arity(&function).is_some())
                        || (provider == "advapi32.dll" && registry::arity(&function).is_some())
                        || (matches!(provider.as_str(), "kernel32.dll" | "kernelbase.dll")
                            && matches!(
                                function.as_str(),
                                "GetErrorMode"
                                    | "GetThreadErrorMode"
                                    | "SetThreadErrorMode"
                                    | "GetTempPath2W"
                                    | "WaitOnAddress"
                                    | "WakeByAddressSingle"
                                    | "WakeByAddressAll"
                                    | "AreFileApisANSI"
                                    | "LCMapStringEx"
                                    | "FlsAlloc"
                                    | "FlsGetValue"
                                    | "FlsSetValue"
                                    | "FlsFree"
                                    | "InitializeCriticalSectionEx"
                                    | "EnumSystemLocalesEx"
                                    | "GetLocaleInfoEx"
                                    | "GetLocaleInfoW"
                                    | "IsValidLocaleName"
                                    | "LocaleNameToLCID"
                                    | "LCIDToLocaleName"
                                    | "GetUserDefaultLocaleName"
                                    | "GetSystemDefaultLocaleName"
                                    | "GetDateFormatEx"
                                    | "GetTimeFormatEx"
                            ))
                        || self.gateways.values().any(|export| {
                            export.split_once('!').is_some_and(|(dll, n)| {
                                dll.eq_ignore_ascii_case(&provider) && n == function
                            })
                        });
                    if !known {
                        self.set_error(mem, 127)?;
                        0
                    } else {
                        self.register(&provider, &function)
                    }
                }
            }
            "InitializeSListHead" => {
                if !args[0].is_multiple_of(16) {
                    return Err("unaligned Win64 SLIST_HEADER".into());
                }
                mem.write(args[0], &[0; 16])?;
                0
            }
            "GetStartupInfoW" | "GetStartupInfoA" => {
                let mut info = [0; 104];
                info[..4].copy_from_slice(&104u32.to_le_bytes());
                mem.write(args[0], &info)?;
                0
            }
            "IsProcessorFeaturePresent" => {
                // Advertise only facilities in this integer startup interpreter.
                (args[0] as u32 == 12) as u64 // Checked NX; no SIMD/AVX/hardware virtualization.
            }
            "SetUnhandledExceptionFilter" => {
                let previous = self.exception_filter;
                self.exception_filter = args[0];
                previous
            }
            "TlsAlloc" => {
                if let Some(index) = self.tls.iter().position(Option::is_none) {
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.check(t.teb + 0x1480 + index as u64 * 8, 8, WRITE)?;
                    }
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.write_int(t.teb + 0x1480 + index as u64 * 8, 8, 0)?;
                    }
                    self.tls[index] = Some(0);
                    index as u64
                } else {
                    self.set_error(mem, 8)?;
                    u32::MAX as u64
                }
            }
            "FlsAlloc" => {
                if let Some(index) = self.fls.iter().position(Option::is_none) {
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.check(t.teb + PAGE + index as u64 * 8, 8, WRITE)?;
                    }
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.write_int(t.teb + PAGE + index as u64 * 8, 8, 0)?;
                    }
                    self.fls[index] = Some((0, args[0]));
                    index as u64
                } else {
                    self.set_error(mem, 8)?;
                    u32::MAX as u64
                }
            }
            "FlsGetValue" | "FlsSetValue" | "FlsFree" => {
                let index = args[0] as u32 as usize;
                if index >= self.fls.len() || self.fls[index].is_none() {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let value = mem.read_int(self.teb() + PAGE + index as u64 * 8, 8)?;
                    let (_, callback) = self.fls[index].unwrap();
                    match name {
                        "FlsGetValue" => {
                            self.set_error(mem, 0)?;
                            value
                        }
                        "FlsSetValue" => {
                            mem.write_int(self.teb() + PAGE + index as u64 * 8, 8, args[1])?;
                            1
                        }
                        _ => {
                            if callback != 0 {
                                for t in self.threads.entries.values().filter(|t| t.exit.is_none())
                                {
                                    if mem.read_int(t.teb + PAGE + index as u64 * 8, 8)? != 0 {
                                        return Err(
                                            "Win64 FlsFree callbacks not implemented".into()
                                        );
                                    }
                                }
                            }
                            self.fls[index] = None;
                            1
                        }
                    }
                }
            }
            "TlsGetValue" | "TlsSetValue" | "TlsFree" => {
                let index = args[0] as u32 as usize;
                if index >= self.tls.len() || self.tls[index].is_none() {
                    self.set_error(mem, 87)?;
                    0
                } else if name == "TlsGetValue" {
                    let value = mem.read_int(self.teb() + 0x1480 + index as u64 * 8, 8)?;
                    self.set_error(mem, 0)?;
                    value
                } else {
                    let value = if name == "TlsFree" { 0 } else { args[1] };
                    mem.write_int(self.teb() + 0x1480 + index as u64 * 8, 8, value)?;
                    self.tls[index] = if name == "TlsFree" { None } else { Some(value) };
                    1
                }
            }
            "InitializeCriticalSection"
            | "InitializeCriticalSectionAndSpinCount"
            | "InitializeCriticalSectionEx" => {
                if arity == 3 && args[2] & !0x01000000 != 0 {
                    return Err("unsupported Win64 critical section flags".into());
                }
                let mut section = [0; 40];
                // No debug object; cooperative ownership and recursion are real.
                section[..8].copy_from_slice(&u64::MAX.to_le_bytes());
                section[8..12].copy_from_slice(&(-1i32).to_le_bytes());
                section[32..40].copy_from_slice(
                    &(args.get(1).copied().unwrap_or(0) as u32 as u64).to_le_bytes(),
                );
                mem.write(args[0], &section)?;
                (name != "InitializeCriticalSection") as u64
            }
            "SetCriticalSectionSpinCount" => {
                mem.check(args[0], 40, READ)?;
                mem.check(args[0] + 32, 8, WRITE)?;
                let old = mem.read_int(args[0] + 32, 8)?;
                mem.write_int(args[0] + 32, 8, args[1])?;
                old
            }
            "EnterCriticalSection"
            | "TryEnterCriticalSection"
            | "LeaveCriticalSection"
            | "DeleteCriticalSection" => {
                let mut section = [0; 40];
                mem.read(args[0], &mut section)?;
                let owner = u64::from_le_bytes(section[16..24].try_into().unwrap());
                let count = u32::from_le_bytes(section[12..16].try_into().unwrap());
                let count = if matches!(name, "EnterCriticalSection" | "TryEnterCriticalSection") {
                    if owner != 0 && owner != self.threads.active {
                        if name == "TryEnterCriticalSection" {
                            returned.regs[0] = 0;
                            *cpu = returned;
                            self.api_calls += 1;
                            self.record(dll, name, &args, 0);
                            return Ok(Control::Returned);
                        }
                        mem.check(args[0], 40, WRITE)?;
                        self.block(threads::Block::Critical(args[0]))?;
                        return Ok(Control::Blocked);
                    }
                    count
                        .checked_add(1)
                        .filter(|&n| n <= i32::MAX as u32)
                        .ok_or("critical section recursion overflow")?
                } else if name == "LeaveCriticalSection" {
                    if owner != self.threads.active || count == 0 {
                        return Err("Win64 critical section not owned".into());
                    }
                    count - 1
                } else {
                    if count != 0 {
                        return Err("deleting an owned Win64 critical section".into());
                    }
                    0
                };
                section[8..12].copy_from_slice(&(count as i32 - 1).to_le_bytes());
                section[12..16].copy_from_slice(&count.to_le_bytes());
                section[16..24].copy_from_slice(
                    &(if count == 0 { 0 } else { self.threads.active }).to_le_bytes(),
                );
                mem.write(args[0], &section)?;
                self.unblock();
                (name == "TryEnterCriticalSection") as u64
            }
            "HeapAlloc" | "HeapFree" | "HeapSize" | "HeapReAlloc" => {
                if args[0] != PROCESS_HEAP || args[1] & !9 != 0 {
                    return Err("unsupported Win64 heap/flags".into());
                }
                self.guest_heap_call(mem, name, &args)?
            }
            "ExitProcess" => {
                self.api_calls += 1;
                self.record(dll, name, &args, args[0] as u32 as u64);
                return Ok(Control::Exited(args[0] as u32));
            }
            _ => unreachable!(),
        };
        returned.regs[0] = ret;
        *cpu = returned;
        self.unblock();
        self.api_calls += 1;
        self.record(dll, name, &args, ret);
        Ok(Control::Returned)
    }
    fn record(&mut self, dll: &str, name: &str, args: &[u64], ret: u64) {
        self.trace_note(format!("{dll}!{name} args={args:x?} -> {ret:#x}"));
    }
    fn trace_note(&mut self, message: String) {
        if self.trace.len() == 4096 {
            self.trace.pop_front();
        }
        self.trace.push_back(message);
    }
}
