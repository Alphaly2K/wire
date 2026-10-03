//! Win32 API stub layer.
//!
//! Every imported function gets a unique guest address in a reserved stub
//! range. When the interpreter's EIP enters that range, control transfers to
//! `dispatch` on the host: the call is logged (name, args, return value), a
//! fake or real result is placed in EAX, the stdcall stack is cleaned up, and
//! execution resumes at the return address. Unimplemented APIs are not silent:
//! they are counted and reported, which is the primary M0 deliverable.

use crate::cpu::Cpu;
use crate::mem::GuestMem;
use crate::thread::{
    target_mentions, Sched, ThreadState, WaitTarget, Wake, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const STUB_BASE: u32 = 0x6fff_0000;
pub const STUB_STRIDE: u32 = 16;
pub const STUB_CAPACITY: usize = 4096;

#[derive(Debug, Clone)]
pub struct ApiEntry {
    pub dll: String,  // lowercased, includes ".dll" (or pseudo-module "wire")
    pub name: String, // export name, or "#<ordinal>"
    pub ordinal: Option<u16>,
}

pub struct ApiTable {
    pub entries: Vec<ApiEntry>,
    by_key: HashMap<(String, String), u32>,
}

impl ApiTable {
    pub fn new() -> ApiTable {
        let mut t = ApiTable {
            entries: Vec::new(),
            by_key: HashMap::new(),
        };
        // index 0: return address pushed for the initial thread entry.
        t.register("wire", Some("ExitEmulation"), None);
        // index 1: return address pushed for worker thread functions.
        t.register("wire", Some("ExitThread"), None);
        t
    }

    pub fn register(&mut self, dll: &str, name: Option<&str>, ordinal: Option<u16>) -> u32 {
        let dll = dll.to_ascii_lowercase();
        let name = name
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("#{}", ordinal.unwrap_or(0)));
        let key = (dll.clone(), name.clone());
        if let Some(&addr) = self.by_key.get(&key) {
            return addr;
        }
        let idx = self.entries.len();
        assert!(idx < STUB_CAPACITY, "API stub table overflow");
        let addr = STUB_BASE + idx as u32 * STUB_STRIDE;
        self.entries.push(ApiEntry {
            dll,
            name,
            ordinal,
        });
        self.by_key.insert(key, addr);
        addr
    }

    pub fn stub_index(&self, addr: u32) -> Option<usize> {
        if addr >= STUB_BASE && addr < STUB_BASE + (self.entries.len() as u32) * STUB_STRIDE {
            Some(((addr - STUB_BASE) / STUB_STRIDE) as usize)
        } else {
            None
        }
    }

    pub fn lookup(&self, dll: &str, name: &str) -> Option<u32> {
        self.by_key
            .get(&(dll.to_ascii_lowercase(), name.to_string()))
            .copied()
    }
}

/// Guest scratch space for fake CRT pointers (starts here, bump-allocated).
pub const SCRATCH_BASE: u32 = 0x00f0_0000;

/// Kernel objects behind guest HANDLE values (M2). Files join this enum when
/// real file I/O lands.
#[derive(Debug)]
pub enum HandleObj {
    Event {
        manual: bool,
        signaled: bool,
    },
    Mutex {
        owner: Option<u32>, // owning tid; recursion via count
        count: u32,
    },
    Semaphore {
        count: i32,
        max: i32,
    },
    Thread {
        tid: u32,
    },
}

/// Mutable host-side state for the emulated process.
pub struct ApiState {
    pub last_error: u32,
    pub start: Instant,
    pub exe_base: u32,
    pub exe_path: String,
    pub cmdline_a: u32,
    pub cmdline_w: u32,
    pub module_handles: HashMap<String, u32>, // dll name (lowercase) -> fake HMODULE
    pub handle_to_dll: HashMap<u32, String>,
    pub next_module_handle: u32,
    pub scratch_top: u32,
    pub argv_ptr: u32,
    pub envp_ptr: u32,
    pub process_heap: u32,
    pub tls: HashMap<u32, u32>,
    pub next_tls: u32,
    pub fls: HashMap<u32, u32>,
    pub next_fls: u32,
    pub handles: HashMap<u32, HandleObj>,
    pub named_objs: HashMap<String, u32>, // event/mutex/semaphore name -> handle
    pub next_handle: u32,
}

impl ApiState {
    pub fn new(exe_base: u32, exe_path: String, cmdline_a: u32, cmdline_w: u32) -> ApiState {
        let mut s = ApiState {
            last_error: 0,
            start: Instant::now(),
            exe_base,
            exe_path,
            cmdline_a,
            cmdline_w,
            module_handles: HashMap::new(),
            handle_to_dll: HashMap::new(),
            next_module_handle: 0x7000_1000,
            scratch_top: SCRATCH_BASE,
            argv_ptr: 0,
            envp_ptr: 0,
            process_heap: 0x7005_0000,
            tls: HashMap::new(),
            next_tls: 1,
            fls: HashMap::new(),
            next_fls: 1,
            handles: HashMap::new(),
            named_objs: HashMap::new(),
            next_handle: 0x100,
        };
        for dll in [
            "kernel32.dll",
            "user32.dll",
            "gdi32.dll",
            "msvcrt.dll",
            "advapi32.dll",
            "ntdll.dll",
        ] {
            s.module_handle_for(dll);
        }
        s
    }

    pub fn alloc_handle(&mut self, obj: HandleObj) -> u32 {
        let h = self.next_handle;
        self.next_handle += 4;
        self.handles.insert(h, obj);
        h
    }

    /// CreateEvent/CreateMutex/CreateSemaphore share name-based dedup:
    /// creating a named object that already exists returns the existing
    /// handle and sets ERROR_ALREADY_EXISTS.
    pub fn create_named(&mut self, name: &str, make: impl FnOnce() -> HandleObj) -> (u32, bool) {
        if !name.is_empty() {
            if let Some(&h) = self.named_objs.get(name) {
                self.last_error = 183; // ERROR_ALREADY_EXISTS
                return (h, true);
            }
        }
        let h = self.alloc_handle(make());
        if !name.is_empty() {
            self.named_objs.insert(name.to_string(), h);
        }
        (h, false)
    }

    pub fn module_handle_for(&mut self, dll: &str) -> u32 {
        let dll = dll.to_ascii_lowercase();
        if let Some(&h) = self.module_handles.get(&dll) {
            return h;
        }
        let h = self.next_module_handle;
        self.next_module_handle += 0x1_0000;
        self.module_handles.insert(dll.clone(), h);
        self.handle_to_dll.insert(h, dll);
        h
    }

    /// Bump-allocate guest scratch memory (for fake CRT structures).
    pub fn scratch_alloc(&mut self, mem: &mut GuestMem, size: u32, align: u32) -> u32 {
        let a = (self.scratch_top + align - 1) & !(align - 1);
        mem.commit(a, size.max(1)).ok();
        self.scratch_top = a + size.max(1);
        a
    }

    /// Lazy argc/argv/envp: argc=1, argv=[cmdline, NULL], envp=[NULL].
    pub fn build_argv(&mut self, mem: &mut GuestMem) -> (u32, u32, u32) {
        if self.argv_ptr == 0 {
            let argv = self.scratch_alloc(mem, 16, 16);
            mem.write_u32(argv, self.cmdline_a).ok();
            mem.write_u32(argv + 4, 0).ok();
            self.argv_ptr = argv;
            self.envp_ptr = self.scratch_alloc(mem, 16, 16); // zero = empty
        }
        (1, self.argv_ptr, self.envp_ptr)
    }
}

pub struct ApiLogger {
    trace: BufWriter<File>,
    pub freq: HashMap<String, u64>,
    pub unimpl_freq: HashMap<String, u64>,
    pub recent: VecDeque<String>,
    pub call_count: u64,
}

impl ApiLogger {
    pub fn new(path: &std::path::Path) -> std::io::Result<ApiLogger> {
        Ok(ApiLogger {
            trace: BufWriter::new(File::create(path)?),
            freq: HashMap::new(),
            unimpl_freq: HashMap::new(),
            recent: VecDeque::new(),
            call_count: 0,
        })
    }

    fn emit(&mut self, line: String) {
        writeln!(self.trace, "{line}").ok();
        self.recent.push_back(line);
        while self.recent.len() > 64 {
            self.recent.pop_front();
        }
    }

    pub fn note(&mut self, msg: &str) {
        self.emit(format!("; {msg}"));
    }

    #[allow(clippy::too_many_arguments)]
    fn log_call(
        &mut self,
        dll: &str,
        name: &str,
        args: &[u32],
        ret: u32,
        implemented: bool,
        note: &str,
    ) {
        self.call_count += 1;
        let key = format!("{dll}!{name}");
        *self.freq.entry(key.clone()).or_insert(0) += 1;
        if !implemented {
            *self.unimpl_freq.entry(key.clone()).or_insert(0) += 1;
        }
        let args_s: Vec<String> = args.iter().map(|a| format!("{a:#x}")).collect();
        let tag = if implemented { "" } else { " [UNIMPLEMENTED]" };
        let note_s = if note.is_empty() {
            String::new()
        } else {
            format!(" ; {note}")
        };
        self.emit(format!(
            "#{:<8} {}({}) = {:#x}{}{}",
            self.call_count,
            key,
            args_s.join(", "),
            ret,
            tag,
            note_s
        ));
    }

    pub fn finish(&mut self) {
        self.emit("; ---- API call frequency (all) ----".into());
        let mut v: Vec<(String, u64)> = self
            .freq
            .iter()
            .map(|(k, c)| (k.clone(), *c))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, c) in &v {
            self.emit(format!("; {c:>8}  {k}"));
        }
        self.emit("; ---- UNIMPLEMENTED API frequency ----".into());
        let mut u: Vec<(String, u64)> = self
            .unimpl_freq
            .iter()
            .map(|(k, c)| (k.clone(), *c))
            .collect();
        u.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, c) in &u {
            self.emit(format!("; {c:>8}  {k}"));
        }
        self.trace.flush().ok();
    }
}

fn filetime_now() -> u64 {
    // 100-ns intervals since 1601-01-01
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    unix / 100 + 116_444_736_000_000_000
}

fn read_str(mem: &GuestMem, addr: u32, wide: bool) -> String {
    if addr == 0 {
        return String::new();
    }
    if wide {
        mem.read_wstr(addr, 260).unwrap_or_else(|_| "<fault>".into())
    } else {
        mem.read_cstr(addr, 260).unwrap_or_else(|_| "<fault>".into())
    }
}

/// stdcall stack cleanup (in bytes) for known APIs. None = unknown (caller is
/// warned that we assume 0, which is only correct for cdecl).
fn stack_args_bytes(dll: &str, name: &str) -> Option<u32> {
    static TABLE: OnceLock<HashMap<(&'static str, &'static str), u32>> = OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut m = HashMap::new();
        let mut add = |dll: &'static str, list: &[(&'static str, u32)]| {
            for (n, b) in list {
                m.insert((dll, *n), *b);
            }
        };
        add(
            "kernel32.dll",
            &[
                ("GetModuleHandleA", 4), ("GetModuleHandleW", 4), ("GetModuleHandleExA", 12),
                ("GetModuleHandleExW", 12), ("GetModuleFileNameA", 12), ("GetModuleFileNameW", 12),
                ("GetProcAddress", 8), ("LoadLibraryA", 4), ("LoadLibraryW", 4),
                ("LoadLibraryExA", 12), ("LoadLibraryExW", 12), ("FreeLibrary", 4),
                ("GetStartupInfoA", 4), ("GetStartupInfoW", 4),
                ("GetCommandLineA", 0), ("GetCommandLineW", 0),
                ("GetStdHandle", 4), ("SetStdHandle", 8), ("GetFileType", 4),
                ("CreateFileA", 28), ("CreateFileW", 28), ("ReadFile", 20), ("WriteFile", 20),
                ("CloseHandle", 4), ("GetFileSize", 8), ("GetFileSizeEx", 8),
                ("SetFilePointer", 16), ("SetFilePointerEx", 20), ("SetEndOfFile", 4),
                ("FlushFileBuffers", 4), ("DeleteFileA", 4), ("DeleteFileW", 4),
                ("CreateDirectoryA", 8), ("CreateDirectoryW", 8),
                ("GetCurrentDirectoryA", 8), ("GetCurrentDirectoryW", 8),
                ("SetCurrentDirectoryA", 4), ("SetCurrentDirectoryW", 4),
                ("GetFullPathNameA", 16), ("GetFullPathNameW", 16),
                ("FindFirstFileA", 8), ("FindFirstFileW", 8), ("FindNextFileA", 8),
                ("FindNextFileW", 8), ("FindClose", 4),
                ("GetFileAttributesA", 4), ("GetFileAttributesW", 4),
                ("SetFileAttributesA", 8), ("SetFileAttributesW", 8),
                ("MoveFileA", 8), ("MoveFileW", 8), ("CopyFileA", 12), ("CopyFileW", 12),
                ("GetTempPathA", 8), ("GetTempPathW", 8),
                ("GetCurrentProcessId", 0), ("GetCurrentThreadId", 0), ("GetCurrentProcess", 0),
                ("GetCurrentThread", 0), ("GetTickCount", 0), ("GetTickCount64", 0),
                ("QueryPerformanceCounter", 4), ("QueryPerformanceFrequency", 4),
                ("GetSystemTimeAsFileTime", 4), ("GetLocalTime", 4), ("GetSystemTime", 4),
                ("SystemTimeToFileTime", 8), ("FileTimeToSystemTime", 8),
                ("FileTimeToLocalFileTime", 8), ("GetTimeZoneInformation", 4),
                ("InitializeCriticalSection", 4), ("EnterCriticalSection", 4),
                ("LeaveCriticalSection", 4), ("DeleteCriticalSection", 4),
                ("InitializeCriticalSectionAndSpinCount", 8), ("TryEnterCriticalSection", 4),
                ("TlsAlloc", 0), ("TlsGetValue", 4), ("TlsSetValue", 8), ("TlsFree", 4),
                ("GetLastError", 0), ("SetLastError", 4),
                ("MultiByteToWideChar", 24), ("WideCharToMultiByte", 32),
                ("VirtualAlloc", 16), ("VirtualFree", 12), ("VirtualProtect", 16),
                ("VirtualQuery", 12),
                ("HeapCreate", 12), ("HeapAlloc", 12), ("HeapFree", 12), ("HeapDestroy", 4),
                ("GetProcessHeap", 0), ("HeapSize", 12), ("HeapReAlloc", 16), ("HeapValidate", 12),
                ("LocalAlloc", 8), ("LocalFree", 4), ("GlobalAlloc", 8), ("GlobalFree", 4),
                ("ExitProcess", 4), ("TerminateProcess", 8),
                ("SetUnhandledExceptionFilter", 4), ("UnhandledExceptionFilter", 4),
                ("IsDebuggerPresent", 0), ("DebugBreak", 0), ("OutputDebugStringA", 4),
                ("OutputDebugStringW", 4),
                ("GetEnvironmentStrings", 0), ("GetEnvironmentStringsW", 0),
                ("FreeEnvironmentStringsA", 4), ("FreeEnvironmentStringsW", 4),
                ("GetEnvironmentVariableA", 12), ("GetEnvironmentVariableW", 12),
                ("SetEnvironmentVariableA", 8), ("SetEnvironmentVariableW", 8),
                ("CreateThread", 24), ("Sleep", 4), ("SwitchToThread", 0), ("ExitThread", 4),
                ("GetSystemInfo", 4), ("GetNativeSystemInfo", 4),
                ("InterlockedIncrement", 4), ("InterlockedDecrement", 4),
                ("InterlockedExchange", 8), ("InterlockedCompareExchange", 12),
                ("GetACP", 0), ("GetOEMCP", 0), ("GetCPInfo", 8), ("IsValidCodePage", 4),
                ("IsDBCSLeadByte", 4), ("IsDBCSLeadByteEx", 8),
                ("GetConsoleCP", 0), ("GetConsoleOutputCP", 0), ("GetCPInfoExA", 12), ("GetCPInfoExW", 12),
                ("GetStringTypeA", 20), ("GetStringTypeW", 16), ("GetStringTypeExA", 20), ("GetStringTypeExW", 20),
                ("GetLocaleInfoA", 16), ("GetLocaleInfoW", 16),
                ("GetUserDefaultLCID", 0), ("GetSystemDefaultLCID", 0), ("GetThreadLocale", 0),
                ("SetThreadLocale", 4), ("EnumSystemLocalesA", 8), ("EnumSystemLocalesW", 8),
                ("CompareStringA", 24), ("CompareStringW", 24), ("LCMapStringA", 24), ("LCMapStringW", 24),
                ("LoadResource", 8), ("FindResourceA", 12), ("FindResourceW", 12),
                ("FindResourceExA", 16), ("FindResourceExW", 16), ("SizeofResource", 8),
                ("LockResource", 4), ("FreeResource", 4),
                ("FormatMessageA", 28), ("FormatMessageW", 28),
                ("CreateEventA", 16), ("CreateEventW", 16), ("SetEvent", 4), ("ResetEvent", 4),
                ("OpenEventA", 12), ("OpenEventW", 12), ("PulseEvent", 4),
                ("CreateMutexA", 12), ("CreateMutexW", 12), ("ReleaseMutex", 4),
                ("OpenMutexA", 12), ("OpenMutexW", 12),
                ("WaitForSingleObject", 8), ("WaitForMultipleObjects", 16),
                ("SleepEx", 8), ("GetExitCodeThread", 8), ("TerminateThread", 8),
                ("CreateSemaphoreA", 16),
                ("GetVersion", 0), ("GetVersionExA", 4), ("GetVersionExW", 4),
                ("VerifyVersionInfoW", 16), ("VerSetConditionMask", 16),
                ("GetComputerNameA", 8), ("GetComputerNameW", 8),
                ("GetUserNameA", 8), ("GetUserNameW", 8),
                ("GetDiskFreeSpaceA", 20), ("GetDiskFreeSpaceW", 20),
                ("GetDriveTypeA", 4), ("GetDriveTypeW", 4),
                ("CreateFileMappingA", 24), ("CreateFileMappingW", 24),
                ("MapViewOfFile", 20), ("UnmapViewOfFile", 4),
                ("DuplicateHandle", 28), ("GetHandleInformation", 8), ("SetHandleInformation", 12),
                ("DeviceIoControl", 32), ("GetOverlappedResult", 16),
                ("IsProcessorFeaturePresent", 4), ("GetSystemDefaultLangID", 0),
                ("GetUserDefaultLangID", 0), ("SetConsoleCtrlHandler", 8),
                ("GetProcessTimes", 20), ("GetProcessWorkingSetSize", 12),
                ("GlobalMemoryStatus", 4), ("GlobalMemoryStatusEx", 4),
                ("EncodePointer", 4), ("DecodePointer", 4),
                ("FlsAlloc", 4), ("FlsGetValue", 4), ("FlsSetValue", 8), ("FlsFree", 4),
                ("IsWow64Process", 8), ("GetLogicalProcessorInformation", 8),
                ("RaiseException", 16), ("RtlUnwind", 16),
                ("InitializeCriticalSectionEx", 12), ("GetCurrentPackageId", 8),
                ("CreateSemaphoreW", 16), ("CreateSemaphoreExW", 24), ("ReleaseSemaphore", 12),
                ("CreateThreadpoolTimer", 12), ("SetThreadpoolTimer", 16),
                ("WaitForThreadpoolTimerCallbacks", 8), ("CloseThreadpoolTimer", 4),
                ("CreateThreadpoolWait", 12), ("SetThreadpoolWait", 12), ("CloseThreadpoolWait", 4),
                ("FlushProcessWriteBuffers", 0), ("FreeLibraryWhenCallbackReturns", 8),
                ("GetCurrentProcessorNumber", 0), ("SetThreadStackGuarantee", 4),
                ("SetDefaultDllDirectories", 4), ("EnumSystemLocalesEx", 16),
                ("CompareStringEx", 36), ("GetDateFormatEx", 24), ("GetLocaleInfoEx", 16),
                ("GetTimeFormatEx", 24), ("GetUserDefaultLocaleName", 8),
                ("IsValidLocaleName", 4), ("LCMapStringEx", 36),
                ("SetThreadPriority", 8), ("GetThreadPriority", 4),
                ("SuspendThread", 4), ("ResumeThread", 4),
                ("OpenProcess", 12), ("GetProcessAffinityMask", 12),
                ("SetThreadAffinityMask", 8), ("SetThreadIdealProcessor", 8),
                ("GlobalAddAtomW", 4), ("GlobalDeleteAtom", 4), ("MulDiv", 12),
                ("FreeConsole", 0), ("AttachConsole", 4), ("GetConsoleMode", 8),
                ("WriteConsoleW", 20), ("SetConsoleTitleW", 4), ("GetConsoleTitleW", 8),
                ("GetConsoleProcessList", 8), ("ReadConsoleW", 20),
                ("GlobalLock", 4), ("GlobalUnlock", 4), ("LocalSize", 4),
                ("LocalLock", 4), ("LocalUnlock", 4), ("GlobalSize", 4),
                ("IsBadReadPtr", 8), ("AreFileApisANSI", 0), ("GetVolumeInformationW", 32),
                ("SearchPathW", 24), ("GetSystemDirectoryW", 8), ("GetWindowsDirectoryW", 8),
                ("GetFileAttributesExW", 12), ("RemoveDirectoryW", 4),
                ("GetExitCodeProcess", 8), ("CreateProcessW", 40), ("CreatePipe", 16),
                ("GetTimeFormatW", 16), ("GetDateFormatW", 16),
                ("lstrcmpW", 8), ("lstrcmpA", 8), ("lstrlenW", 4), ("lstrlenA", 4),
                ("IsValidLocale", 8), ("HeapSetInformation", 16), ("HeapQueryInformation", 20),
                ("HeapCompact", 8), ("HeapWalk", 8), ("GetLargePageMinimum", 0),
            ],
        );
        add(
            "user32.dll",
            &[
                ("RegisterClassA", 4), ("RegisterClassW", 4), ("RegisterClassExA", 4),
                ("RegisterClassExW", 4), ("UnregisterClassA", 8), ("UnregisterClassW", 8),
                ("CreateWindowExA", 48), ("CreateWindowExW", 48), ("DestroyWindow", 4),
                ("DefWindowProcA", 16), ("DefWindowProcW", 16),
                ("ShowWindow", 8), ("UpdateWindow", 4), ("IsWindow", 4), ("IsWindowVisible", 4),
                ("IsIconic", 4), ("IsZoomed", 4), ("EnableWindow", 8),
                ("GetProcessWindowStation", 0), ("GetUserObjectInformationW", 20),
                ("GetUserObjectInformationA", 20), ("GetLastActivePopup", 4),
                ("GetMessageA", 16), ("GetMessageW", 16), ("PeekMessageA", 20), ("PeekMessageW", 20),
                ("TranslateMessage", 4), ("DispatchMessageA", 4), ("DispatchMessageW", 4),
                ("PostQuitMessage", 4), ("SendMessageA", 16), ("SendMessageW", 16),
                ("PostMessageA", 16), ("PostMessageW", 16), ("PostThreadMessageA", 16),
                ("LoadIconA", 8), ("LoadIconW", 8), ("LoadCursorA", 8), ("LoadCursorW", 8),
                ("LoadImageA", 24), ("LoadImageW", 24), ("SetCursor", 4), ("GetCursor", 0),
                ("GetDC", 4), ("GetWindowDC", 4), ("ReleaseDC", 8),
                ("BeginPaint", 8), ("EndPaint", 8), ("InvalidateRect", 12), ("ValidateRect", 8),
                ("GetClientRect", 8), ("GetWindowRect", 8), ("SetWindowTextA", 8), ("SetWindowTextW", 8),
                ("GetWindowTextA", 12), ("GetWindowTextW", 12),
                ("MessageBoxA", 16), ("MessageBoxW", 16), ("MessageBoxExA", 20), ("MessageBoxExW", 20),
                ("SetTimer", 16), ("KillTimer", 8),
                ("GetSystemMetrics", 4), ("SystemParametersInfoA", 16), ("SystemParametersInfoW", 16),
                ("GetForegroundWindow", 0), ("SetForegroundWindow", 4),
                ("SetFocus", 4), ("GetFocus", 0), ("GetActiveWindow", 0), ("SetActiveWindow", 4),
                ("MoveWindow", 24), ("SetWindowPos", 28), ("BringWindowToTop", 4),
                ("ClientToScreen", 8), ("ScreenToClient", 8), ("WindowFromPoint", 8),
                ("GetCursorPos", 4), ("SetCursorPos", 8), ("ClipCursor", 4), ("ShowCursor", 4),
                ("LoadAcceleratorsA", 8), ("LoadAcceleratorsW", 8),
                ("TranslateAcceleratorA", 12), ("TranslateAcceleratorW", 12),
                ("AdjustWindowRect", 12), ("AdjustWindowRectEx", 16),
                ("GetDesktopWindow", 0), ("GetShellWindow", 0), ("GetParent", 4), ("SetParent", 8),
                ("DrawIcon", 12), ("FillRect", 12), ("FrameRect", 12), ("InvertRect", 8),
                ("GetKeyState", 4), ("GetAsyncKeyState", 4), ("GetKeyboardState", 4),
                ("SetKeyboardState", 4), ("VkKeyScanA", 4), ("VkKeyScanW", 4),
                ("MapVirtualKeyA", 8), ("MapVirtualKeyW", 8), ("GetKeyNameTextA", 12),
                ("RegisterHotKey", 16), ("UnregisterHotKey", 8),
                ("CharNextA", 4), ("CharNextW", 4), ("CharPrevA", 8), ("CharPrevW", 8),
                ("CharUpperA", 4), ("CharUpperW", 4), ("CharLowerA", 4), ("CharLowerW", 4),
                ("wsprintfA", 0), ("wsprintfW", 0), // cdecl varargs
                ("wvsprintfA", 12), ("wvsprintfW", 12),
                ("DrawTextA", 20), ("DrawTextW", 20), ("DrawTextExA", 24), ("DrawTextExW", 24),
                ("GetWindowLongA", 8), ("GetWindowLongW", 8), ("SetWindowLongA", 12), ("SetWindowLongW", 12),
                ("GetClassLongA", 8), ("GetClassLongW", 8),
                ("DefDlgProcA", 16), ("DefDlgProcW", 16),
                ("DialogBoxParamA", 20), ("DialogBoxParamW", 20),
                ("CreateDialogParamA", 20), ("CreateDialogParamW", 20),
                ("EndDialog", 8), ("GetDlgItem", 8), ("SetDlgItemTextA", 12), ("SetDlgItemTextW", 12),
                ("GetDlgItemTextA", 16), ("GetDlgItemTextW", 16),
                ("GetMenu", 4), ("SetMenu", 8), ("LoadMenuA", 8), ("LoadMenuW", 8),
                ("TrackPopupMenu", 28), ("GetSystemMenu", 8),
                ("SetRect", 20), ("CopyRect", 8), ("InflateRect", 12), ("OffsetRect", 12),
                ("PtInRect", 12), ("IsRectEmpty", 4), ("UnionRect", 12), ("IntersectRect", 12),
                ("GetSysColor", 4), ("GetSysColorBrush", 4),
                ("EnumWindows", 8), ("FindWindowA", 8), ("FindWindowW", 8),
                ("GetWindowThreadProcessId", 8), ("AttachThreadInput", 12),
                ("RegisterWindowMessageA", 4), ("RegisterWindowMessageW", 4),
                ("RedrawWindow", 16), ("GetUpdateRect", 12), ("MapWindowPoints", 16),
                ("ReleaseCapture", 0), ("SetCapture", 4), ("GetCapture", 0),
                ("MonitorFromWindow", 8), ("GetMonitorInfoA", 8), ("GetMonitorInfoW", 8),
                ("EnumDisplaySettingsA", 12), ("EnumDisplaySettingsW", 12),
                ("ChangeDisplaySettingsA", 8), ("ChangeDisplaySettingsW", 8),
                ("SetWindowsHookExA", 16), ("SetWindowsHookExW", 16),
                ("UnhookWindowsHookEx", 4), ("CallNextHookEx", 16),
                ("GetKeyboardLayout", 4), ("GetKeyboardLayoutList", 8), ("ActivateKeyboardLayout", 8),
                ("ToAscii", 20), ("ToUnicode", 24), ("GetDoubleClickTime", 0),
            ],
        );
        add(
            "gdi32.dll",
            &[
                ("CreateCompatibleDC", 4), ("DeleteDC", 4), ("CreateDCA", 16), ("CreateDCW", 16),
                ("CreateDIBSection", 24), ("CreateBitmap", 20), ("CreateCompatibleBitmap", 12),
                ("CreateBitmapIndirect", 4), ("DeleteObject", 4), ("SelectObject", 8),
                ("GetObjectA", 12), ("GetObjectW", 12), ("GetCurrentObject", 8),
                ("BitBlt", 36), ("StretchBlt", 44), ("PatBlt", 24), ("MaskBlt", 52),
                ("TransparentBlt", 44), ("AlphaBlend", 44), ("GradientFill", 24),
                ("GetDIBits", 28), ("SetDIBits", 28), ("SetDIBitsToDevice", 48), ("StretchDIBits", 52),
                ("CreateFontIndirectA", 4), ("CreateFontIndirectW", 4),
                ("CreateFontA", 56), ("CreateFontW", 56),
                ("CreatePen", 12), ("CreatePenIndirect", 12), ("CreateSolidBrush", 4),
                ("CreateHatchBrush", 8), ("CreatePatternBrush", 4), ("CreateBrushIndirect", 4),
                ("CreateRectRgn", 16), ("CreateRectRgnIndirect", 4), ("CreateEllipticRgn", 16),
                ("CreateRoundRectRgn", 24), ("CombineRgn", 20), ("SelectClipRgn", 8),
                ("SetTextColor", 8), ("GetTextColor", 4), ("SetBkColor", 8), ("GetBkColor", 4),
                ("SetBkMode", 8), ("GetBkMode", 4),
                ("TextOutA", 20), ("TextOutW", 20), ("ExtTextOutA", 32), ("ExtTextOutW", 32),
                ("GetTextExtentPoint32A", 16), ("GetTextExtentPoint32W", 16),
                ("GetTextExtentExPointA", 28), ("GetTextExtentExPointW", 28),
                ("GetCharWidthA", 16), ("GetCharWidthW", 16), ("GetCharWidth32A", 16), ("GetCharWidth32W", 16),
                ("GetTextMetricsA", 8), ("GetTextMetricsW", 8), ("GetOutlineTextMetricsA", 12),
                ("GetGlyphOutlineA", 28), ("GetGlyphOutlineW", 28),
                ("SetTextAlign", 8), ("GetTextAlign", 4), ("SetTextCharacterExtra", 8),
                ("GetStockObject", 4), ("SetPixel", 16), ("GetPixel", 12), ("SetPixelV", 16),
                ("Rectangle", 20), ("Ellipse", 20), ("RoundRect", 28), ("Polygon", 12),
                ("Polyline", 12), ("MoveToEx", 16), ("LineTo", 12), ("Arc", 36), ("Pie", 36),
                ("SetROP2", 8), ("GetROP2", 4), ("SetPolyFillMode", 8),
                ("SetMapMode", 8), ("GetMapMode", 4),
                ("SetViewportOrgEx", 16), ("GetViewportOrgEx", 8), ("SetWindowOrgEx", 16),
                ("GetWindowOrgEx", 8), ("SetViewportExtEx", 16), ("SetWindowExtEx", 16),
                ("DPtoLP", 12), ("LPtoDP", 12),
                ("GetDeviceCaps", 8), ("GetDCOrgEx", 8),
                ("SelectPalette", 12), ("RealizePalette", 4), ("CreatePalette", 4),
                ("SetPaletteEntries", 16), ("GetPaletteEntries", 16),
                ("SetStretchBltMode", 8), ("GetStretchBltMode", 4),
                ("SetBrushOrgEx", 16), ("UnrealizeObject", 4),
                ("GetClipBox", 8), ("IntersectClipRect", 16), ("ExcludeClipRect", 16),
                ("SaveDC", 4), ("RestoreDC", 8),
                ("GetTextFaceA", 12), ("GetTextFaceW", 12),
                ("EnumFontFamiliesExA", 20), ("EnumFontFamiliesExW", 20),
                ("GetFontData", 20), ("AddFontResourceA", 4), ("AddFontResourceW", 4),
                ("RemoveFontResourceA", 4), ("RemoveFontResourceW", 4),
                ("CreateHalftonePalette", 4), ("SetColorAdjustment", 8),
                ("StartDocA", 8), ("StartDocW", 8), ("EndDoc", 4), ("StartPage", 4), ("EndPage", 4),
                ("GdiFlush", 0), ("GetPixelFormat", 4), ("SetPixelFormat", 12),
                ("SwapBuffers", 4), ("ChoosePixelFormat", 8),
            ],
        );
        add(
            "advapi32.dll",
            &[
                ("RegOpenKeyExA", 20), ("RegOpenKeyExW", 20), ("RegCloseKey", 4),
                ("RegQueryValueExA", 24), ("RegQueryValueExW", 24), ("RegSetValueExA", 24),
                ("RegSetValueExW", 24), ("RegCreateKeyExA", 36), ("RegCreateKeyExW", 36),
                ("RegDeleteKeyA", 8), ("RegDeleteKeyW", 8), ("RegEnumKeyExA", 32),
                ("RegEnumValueA", 32), ("RegQueryInfoKeyA", 48),
                ("GetUserNameA", 8), ("GetUserNameW", 8),
                ("OpenProcessToken", 12), ("GetTokenInformation", 20),
                ("LookupPrivilegeValueA", 12), ("AdjustTokenPrivileges", 24),
            ],
        );
        add(
            "version.dll",
            &[
                ("GetFileVersionInfoA", 16), ("GetFileVersionInfoW", 16),
                ("GetFileVersionInfoSizeA", 8), ("GetFileVersionInfoSizeW", 8),
                ("VerQueryValueA", 16), ("VerQueryValueW", 16),
            ],
        );
        add(
            "winmm.dll",
            &[
                ("timeGetTime", 0), ("timeBeginPeriod", 4), ("timeEndPeriod", 4),
                ("timeGetDevCaps", 8), ("timeSetEvent", 20), ("timeKillEvent", 4),
                ("waveOutGetNumDevs", 0), ("waveOutOpen", 24), ("waveOutClose", 4),
                ("waveOutPrepareHeader", 12), ("waveOutUnprepareHeader", 12),
                ("waveOutWrite", 12), ("waveOutReset", 4), ("waveOutPause", 4),
                ("waveOutRestart", 4), ("waveOutGetPosition", 12), ("waveOutSetVolume", 8),
                ("mciSendStringA", 16), ("mciSendStringW", 16),
                ("PlaySoundA", 12), ("PlaySoundW", 12),
                ("midiOutGetNumDevs", 0), ("joyGetNumDevs", 0),
            ],
        );
        add(
            "imm32.dll",
            &[
                ("ImmGetContext", 4), ("ImmReleaseContext", 8), ("ImmGetCompositionStringA", 16),
                ("ImmGetCompositionStringW", 16), ("ImmSetCompositionStringA", 24),
                ("ImmSetCompositionStringW", 24), ("ImmNotifyIME", 16),
                ("ImmGetDefaultIMEWnd", 4), ("ImmIsIME", 4), ("ImmGetOpenStatus", 4),
                ("ImmSetOpenStatus", 8), ("ImmAssociateContext", 8), ("ImmAssociateContextEx", 12),
            ],
        );
        add(
            "ole32.dll",
            &[
                ("CoInitialize", 4), ("CoInitializeEx", 8), ("CoUninitialize", 0),
                ("CoCreateInstance", 20), ("CoTaskMemAlloc", 4), ("CoTaskMemFree", 4),
                ("OleInitialize", 4), ("OleUninitialize", 0),
                ("CreateStreamOnHGlobal", 12), ("GetHGlobalFromStream", 8),
                ("CLSIDFromString", 8), ("StringFromCLSID", 8),
            ],
        );
        add(
            "shell32.dll",
            &[
                ("ShellExecuteA", 24), ("ShellExecuteW", 24), ("SHGetFolderPathA", 20),
                ("SHGetFolderPathW", 20), ("SHGetSpecialFolderPathA", 16),
                ("SHBrowseForFolderA", 4), ("SHGetPathFromIDListA", 8),
                ("DragAcceptFiles", 8), ("DragQueryFileA", 16), ("DragQueryFileW", 16),
                ("DragFinish", 4), ("CommandLineToArgvW", 8),
            ],
        );
        add(
            "comctl32.dll",
            &[("InitCommonControls", 0), ("InitCommonControlsEx", 4)],
        );
        add(
            "shlwapi.dll",
            &[
                ("PathCombineA", 12), ("PathCombineW", 12), ("PathFileExistsA", 4),
                ("PathFileExistsW", 4), ("PathFindExtensionA", 4), ("PathFindFileNameA", 4),
                ("PathRemoveExtensionA", 4), ("PathRemoveFileSpecA", 4),
                ("StrStrIA", 8), ("StrStrIW", 8),
            ],
        );
        add(
            "ws2_32.dll",
            &[("WSAStartup", 8), ("WSACleanup", 0), ("socket", 12), ("closesocket", 4)],
        );
        add(
            "ddraw.dll",
            &[("DirectDrawCreate", 12), ("DirectDrawCreateEx", 16), ("DirectDrawEnumerateA", 8)],
        );
        add(
            "dsound.dll",
            &[("DirectSoundCreate", 12), ("DirectSoundCreate8", 12), ("DirectSoundEnumerateA", 8)],
        );
        add(
            "dinput.dll",
            &[("DirectInputCreateA", 16), ("DirectInputCreateW", 16), ("DirectInput8Create", 20)],
        );
        m
    });
    if let Some(&b) = t.get(&(dll, name)) {
        return Some(b);
    }
    // CRT DLLs are cdecl: caller cleans the stack. (msvcr## = MSVC runtime,
    // api-ms-win-crt-* / ucrtbase = UCRT.)
    if dll == "msvcrt.dll"
        || dll == "crtdll.dll"
        || dll == "ucrtbase.dll"
        || dll.starts_with("api-ms-win-crt-")
        || (dll.starts_with("msvcr") && dll.ends_with(".dll"))
    {
        return Some(0);
    }
    None
}

/// --- kernel-object wait helpers (M2) ---

/// Non-consuming readiness test (used by wait-all and by acquire itself).
fn obj_ready(state: &ApiState, sched: &Sched, h: u32) -> bool {
    match state.handles.get(&h) {
        Some(HandleObj::Event { signaled, .. }) => *signaled,
        Some(HandleObj::Mutex { owner, .. }) => {
            owner.is_none() || *owner == Some(sched.current_tid())
        }
        Some(HandleObj::Semaphore { count, .. }) => *count > 0,
        Some(HandleObj::Thread { tid }) => sched
            .threads
            .iter()
            .any(|t| t.tid == *tid && matches!(t.state, ThreadState::Exited(_))),
        None => false,
    }
}

/// Consuming acquire: events clear their signal (auto-reset), mutexes take
/// ownership, semaphores decrement. Thread handles have nothing to consume.
fn obj_acquire(state: &mut ApiState, sched: &Sched, h: u32) -> bool {
    match state.handles.get_mut(&h) {
        Some(HandleObj::Event { manual, signaled }) => {
            if *signaled {
                if !*manual {
                    *signaled = false;
                }
                true
            } else {
                false
            }
        }
        Some(HandleObj::Mutex { owner, count }) => {
            let me = sched.current_tid();
            match *owner {
                None => {
                    *owner = Some(me);
                    *count = 1;
                    true
                }
                Some(o) if o == me => {
                    *count += 1;
                    true
                }
                _ => false,
            }
        }
        Some(HandleObj::Semaphore { count, .. }) => {
            if *count > 0 {
                *count -= 1;
                true
            } else {
                false
            }
        }
        Some(HandleObj::Thread { tid }) => {
            let tid = *tid;
            sched
                .threads
                .iter()
                .any(|t| t.tid == tid && matches!(t.state, ThreadState::Exited(_)))
        }
        None => false,
    }
}

/// Wake up to `max` threads waiting on object `h` so they re-test it.
fn wake_obj_waiters(sched: &mut Sched, h: u32, max: usize) -> usize {
    let mut n = 0;
    for i in 0..sched.threads.len() {
        if n >= max {
            break;
        }
        let hit = matches!(
            &sched.threads[i].state,
            ThreadState::Waiting { target, .. } if target_mentions(target, h)
        );
        if hit {
            sched.wake_thread(i, Wake::Retest);
            n += 1;
        }
    }
    n
}

/// Execute one API call on behalf of the guest. EIP currently points at the
/// stub address; the guest stack has [ret][args...].
#[allow(clippy::too_many_arguments)]
pub fn dispatch(
    idx: usize,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(), String> {
    let entry = api.entries[idx].clone();
    let dll = entry.dll.as_str();
    let name = entry.name.as_str();
    let esp = cpu.esp();
    let mut args = [0u32; 6];
    for (i, a) in args.iter_mut().enumerate() {
        *a = mem
            .read_u32(esp.wrapping_add(4 + 4 * i as u32))
            .unwrap_or(0xdead_beef);
    }
    let mut implemented = true;
    let mut note = String::new();
    let ret: u32;
    match (dll, name) {
        ("wire", "ExitEmulation") => {
            log.log_call(dll, name, &args[..1], 0, true, "initial thread returned");
            cpu.halted = true;
            cpu.exit_code = Some(0);
            return Ok(());
        }
        ("wire", "ExitThread") => {
            // A worker thread function returned; its EAX is the exit code.
            let code = cpu.regs[0];
            log.log_call(dll, name, &args[..1], 0, true, "thread function returned");
            sched.exit_current(cpu, code)?;
            return Ok(());
        }
        ("kernel32.dll", "ExitProcess") => {
            log.log_call(dll, name, &args[..1], 0, true, "process exit");
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            return Ok(());
        }
        ("kernel32.dll", "ExitThread") => {
            log.log_call(dll, name, &args[..1], 0, true, "thread exit");
            sched.exit_current(cpu, args[0])?;
            return Ok(());
        }
        ("kernel32.dll", "CreateThread") => {
            let (entry, param, flags) = (args[2], args[3], args[4]);
            let tid = sched.alloc_tid();
            let exit_stub = api.lookup("wire", "ExitThread").unwrap();
            let handle = state.alloc_handle(HandleObj::Thread { tid });
            sched.create_thread(mem, entry, param, flags & 4 != 0, tid, handle, exit_stub)?;
            if args[5] != 0 {
                mem.write_u32(args[5], tid).map_err(|e| e.to_string())?;
            }
            note = format!(
                "tid {tid} entry={entry:#x} param={param:#x}{}",
                if flags & 4 != 0 { " (suspended)" } else { "" }
            );
            ret = handle;
        }
        ("kernel32.dll", "GetCurrentThread") => ret = 0xffff_fffe,
        ("kernel32.dll", "GetCurrentThreadId") => ret = sched.current_tid(),
        ("kernel32.dll", "GetExitCodeThread") => {
            let (code, running) = match sched.thread_index_by_handle(args[0]) {
                Some(i) => match sched.threads[i].state {
                    ThreadState::Exited(c) => (c, false),
                    _ => (0x103, true), // STILL_ACTIVE
                },
                None => (0, false),
            };
            mem.write_u32(args[1], code).map_err(|e| e.to_string())?;
            ret = 1;
            note = if running { "STILL_ACTIVE".into() } else { format!("exited, code {code:#x}") };
        }
        ("kernel32.dll", "SuspendThread") => {
            if let Some(i) = sched.thread_index_by_handle(args[0]) {
                if matches!(sched.threads[i].state, ThreadState::Runnable) {
                    sched.threads[i].state = ThreadState::Suspended;
                }
                ret = 0;
            } else {
                state.last_error = 6;
                ret = u32::MAX;
            }
        }
        ("kernel32.dll", "ResumeThread") => {
            if let Some(i) = sched.thread_index_by_handle(args[0]) {
                if sched.threads[i].state == ThreadState::Suspended {
                    sched.threads[i].state = ThreadState::Runnable;
                }
                ret = 0;
            } else {
                state.last_error = 6;
                ret = u32::MAX;
            }
        }
        ("kernel32.dll", "SwitchToThread") => {
            ret = sched.round_robin(cpu) as u32;
        }
        ("kernel32.dll", "Sleep") => {
            if sched.take_wake().is_some() {
                ret = 0; // void; woke after the full delay
            } else if args[0] == 0 {
                sched.round_robin(cpu); // yield
                ret = 0;
            } else {
                let d = sched.arm_deadline(args[0]);
                log.note(&format!(
                    "thread {} sleeps {}ms",
                    sched.current_tid(),
                    args[0]
                ));
                sched.block_current(cpu, WaitTarget::Sleep, d)?;
                return Ok(());
            }
        }
        ("kernel32.dll", "SleepEx") => {
            if sched.take_wake().is_some() {
                ret = 0;
            } else if args[0] == 0 {
                sched.round_robin(cpu);
                ret = 0;
            } else {
                let d = sched.arm_deadline(args[0]);
                sched.block_current(cpu, WaitTarget::Sleep, d)?;
                return Ok(());
            }
        }
        // --- events ---
        ("kernel32.dll", "CreateEventA") | ("kernel32.dll", "CreateEventW") => {
            let wide = name.ends_with('W');
            let nm = read_str(mem, args[3], wide);
            let (h, existed) = state.create_named(&nm, || HandleObj::Event {
                manual: args[1] != 0,
                signaled: args[2] != 0,
            });
            note = format!(
                "event \"{nm}\" manual={} signaled={}{}",
                args[1] != 0,
                args[2] != 0,
                if existed { " (already exists)" } else { "" }
            );
            ret = h;
        }
        ("kernel32.dll", "OpenEventA") | ("kernel32.dll", "OpenEventW") => {
            let wide = name.ends_with('W');
            let nm = read_str(mem, args[2], wide);
            match state.named_objs.get(&nm) {
                Some(&h) => {
                    ret = h;
                    note = format!("event \"{nm}\"");
                }
                None => {
                    state.last_error = 2; // ERROR_FILE_NOT_FOUND
                    ret = 0;
                    note = format!("event \"{nm}\" not found");
                }
            }
        }
        ("kernel32.dll", "SetEvent") => {
            match state.handles.get_mut(&args[0]) {
                Some(HandleObj::Event { manual, signaled }) => {
                    let manual = *manual;
                    *signaled = true;
                    let woken = wake_obj_waiters(sched, args[0], if manual { usize::MAX } else { 1 });
                    note = format!("woke {woken} waiter(s)");
                    ret = 1;
                }
                _ => {
                    state.last_error = 6;
                    ret = 0;
                    note = "bad event handle".into();
                }
            }
        }
        ("kernel32.dll", "ResetEvent") => {
            match state.handles.get_mut(&args[0]) {
                Some(HandleObj::Event { signaled, .. }) => {
                    *signaled = false;
                    ret = 1;
                }
                _ => {
                    state.last_error = 6;
                    ret = 0;
                }
            }
        }
        ("kernel32.dll", "PulseEvent") => {
            match state.handles.get_mut(&args[0]) {
                Some(HandleObj::Event { manual, signaled }) => {
                    let manual = *manual;
                    *signaled = false;
                    let woken = wake_obj_waiters(sched, args[0], if manual { usize::MAX } else { 1 });
                    note = format!("woke {woken} waiter(s)");
                    ret = 1;
                }
                _ => {
                    state.last_error = 6;
                    ret = 0;
                }
            }
        }
        // --- mutexes ---
        ("kernel32.dll", "CreateMutexA") | ("kernel32.dll", "CreateMutexW") => {
            let wide = name.ends_with('W');
            let nm = read_str(mem, args[2], wide);
            let me = sched.current_tid();
            let own = args[1] != 0;
            let (h, existed) = state.create_named(&nm, || HandleObj::Mutex {
                owner: if own { Some(me) } else { None },
                count: if own { 1 } else { 0 },
            });
            note = format!("mutex \"{nm}\"{}", if existed { " (already exists)" } else { "" });
            ret = h;
        }
        ("kernel32.dll", "ReleaseMutex") => {
            let me = sched.current_tid();
            match state.handles.get_mut(&args[0]) {
                Some(HandleObj::Mutex { owner, count }) if *owner == Some(me) => {
                    *count -= 1;
                    if *count == 0 {
                        *owner = None;
                        wake_obj_waiters(sched, args[0], 1);
                    }
                    ret = 1;
                }
                Some(HandleObj::Mutex { .. }) => {
                    state.last_error = 288; // ERROR_NOT_OWNER
                    ret = 0;
                    note = "not the owner".into();
                }
                _ => {
                    state.last_error = 6;
                    ret = 0;
                    note = "bad mutex handle".into();
                }
            }
        }
        // --- semaphores ---
        ("kernel32.dll", "CreateSemaphoreW") | ("kernel32.dll", "CreateSemaphoreA") => {
            let wide = name.ends_with('W');
            let nm = read_str(mem, args[3], wide);
            let (initial, max) = (args[1] as i32, args[2] as i32);
            let (h, existed) = state.create_named(&nm, || HandleObj::Semaphore {
                count: initial,
                max,
            });
            note = format!("semaphore \"{nm}\" {initial}/{max}{}", if existed { " (already exists)" } else { "" });
            ret = h;
        }
        ("kernel32.dll", "ReleaseSemaphore") => {
            match state.handles.get_mut(&args[0]) {
                Some(HandleObj::Semaphore { count, max }) => {
                    let prev = *count;
                    let n = args[1] as i32;
                    *count = (*count + n).min(*max);
                    if args[2] != 0 {
                        mem.write_u32(args[2], prev as u32).map_err(|e| e.to_string())?;
                    }
                    let woken = wake_obj_waiters(sched, args[0], n.max(0) as usize);
                    note = format!("count {prev} -> {}, woke {woken}", *count);
                    ret = 1;
                }
                _ => {
                    state.last_error = 6;
                    ret = 0;
                    note = "bad semaphore handle".into();
                }
            }
        }
        // --- waits ---
        ("kernel32.dll", "WaitForSingleObject") => {
            let (h, timeout) = (args[0], args[1]);
            match sched.take_wake() {
                Some(Wake::Completed(r)) => {
                    ret = r;
                    note = "wait completed".into();
                }
                wake => {
                    if obj_acquire(state, sched, h) {
                        sched.clear_deadline();
                        ret = WAIT_OBJECT_0;
                    } else if !state.handles.contains_key(&h) {
                        sched.clear_deadline();
                        state.last_error = 6;
                        ret = 0xffff_ffff; // WAIT_FAILED
                        note = "invalid handle".into();
                    } else if timeout == 0 && wake.is_none() {
                        ret = WAIT_TIMEOUT;
                        note = "poll, not ready".into();
                    } else {
                        let d = sched.arm_deadline(timeout);
                        log.note(&format!(
                            "thread {} blocks in WaitForSingleObject({h:#x}, {}ms)",
                            sched.current_tid(),
                            if timeout == u32::MAX { "inf".into() } else { timeout.to_string() }
                        ));
                        sched.block_current(cpu, WaitTarget::Object(h), d)?;
                        return Ok(());
                    }
                }
            }
        }
        ("kernel32.dll", "WaitForMultipleObjects") => {
            let (n, ptr, wait_all, timeout) = (args[0], args[1], args[2] != 0, args[3]);
            let mut hs = Vec::new();
            for i in 0..n.min(64) {
                hs.push(mem.read_u32(ptr + 4 * i).map_err(|e| e.to_string())?);
            }
            match sched.take_wake() {
                Some(Wake::Completed(r)) => {
                    ret = r;
                    note = "wait completed".into();
                }
                wake => {
                    let mut got = None;
                    if wait_all {
                        if hs.iter().all(|&h| obj_ready(state, sched, h)) {
                            for &h in &hs {
                                obj_acquire(state, sched, h);
                            }
                            got = Some(0);
                        }
                    } else {
                        for (i, &h) in hs.iter().enumerate() {
                            if obj_acquire(state, sched, h) {
                                got = Some(i as u32);
                                break;
                            }
                        }
                    }
                    match got {
                        Some(i) => {
                            sched.clear_deadline();
                            ret = WAIT_OBJECT_0 + i;
                        }
                        None if timeout == 0 && wake.is_none() => {
                            ret = WAIT_TIMEOUT;
                            note = "poll, not ready".into();
                        }
                        None => {
                            let d = sched.arm_deadline(timeout);
                            log.note(&format!(
                                "thread {} blocks in WaitForMultipleObjects({hs:?}, all={wait_all})",
                                sched.current_tid()
                            ));
                            sched.block_current(cpu, WaitTarget::Multi(hs, wait_all), d)?;
                            return Ok(());
                        }
                    }
                }
            }
        }
        ("kernel32.dll", "CloseHandle") => {
            if state.handles.remove(&args[0]).is_some() {
                ret = 1;
            } else {
                state.last_error = 6;
                ret = 0;
                note = "unknown handle".into();
            }
        }
        ("kernel32.dll", "SetThreadPriority") => {
            ret = 1;
            note = "no-op (cooperative scheduler)".into();
        }
        ("kernel32.dll", "GetThreadPriority") => ret = 0, // THREAD_PRIORITY_NORMAL
        ("kernel32.dll", "GetLastError") => ret = state.last_error,
        ("kernel32.dll", "SetLastError") => {
            state.last_error = args[0];
            ret = 0;
        }
        ("kernel32.dll", "GetVersion") => {
            ret = 0x1db1_0106;
            note = "fake Windows 7 (6.1.7601)".into();
        }
        ("kernel32.dll", "GetCurrentProcess") => ret = 0xffff_ffff,
        ("kernel32.dll", "GetCurrentProcessId") => {
            ret = 4242;
            note = "fake pid".into();
        }
        ("kernel32.dll", "GetTickCount") => {
            ret = state.start.elapsed().as_millis() as u32;
        }
        ("kernel32.dll", "GetCommandLineA") => ret = state.cmdline_a,
        ("kernel32.dll", "GetCommandLineW") => ret = state.cmdline_w,
        ("kernel32.dll", "GetACP") => {
            ret = 932;
            note = "fake CP932 (ja-JP)".into();
        }
        ("kernel32.dll", "GetOEMCP") => {
            ret = 932;
            note = "fake CP932 (ja-JP)".into();
        }
        ("kernel32.dll", "IsDBCSLeadByte") => {
            let b = args[0] as u8;
            ret = ((0x81..=0x9f).contains(&b) || (0xe0..=0xfc).contains(&b)) as u32;
            note = "CP932 lead-byte ranges".into();
        }
        ("kernel32.dll", "IsDBCSLeadByteEx") => {
            let b = args[1] as u8;
            ret = ((0x81..=0x9f).contains(&b) || (0xe0..=0xfc).contains(&b)) as u32;
            note = "CP932 lead-byte ranges (codepage arg ignored)".into();
        }
        ("kernel32.dll", "GetConsoleCP") | ("kernel32.dll", "GetConsoleOutputCP") => {
            ret = 932;
            note = "fake CP932".into();
        }
        ("kernel32.dll", "IsDebuggerPresent") => ret = 0,
        ("kernel32.dll", "GetSystemTimeAsFileTime") => {
            mem.write_u64(args[0], filetime_now())
                .map_err(|e| e.to_string())?;
            ret = 0;
        }
        ("kernel32.dll", "QueryPerformanceCounter") => {
            let v = state.start.elapsed().as_nanos() as u64 * 10;
            mem.write_u64(args[0], v).map_err(|e| e.to_string())?;
            ret = 1;
        }
        ("kernel32.dll", "QueryPerformanceFrequency") => {
            mem.write_u64(args[0], 10_000_000).map_err(|e| e.to_string())?;
            ret = 1;
        }
        ("kernel32.dll", "GetStartupInfoA") | ("kernel32.dll", "GetStartupInfoW") => {
            let mut si = [0u8; 68];
            si[0..4].copy_from_slice(&68u32.to_le_bytes());
            mem.write_bytes(args[0], &si).map_err(|e| e.to_string())?;
            note = "zeroed STARTUPINFO".into();
            ret = 0;
        }
        ("kernel32.dll", "GetModuleHandleA") | ("kernel32.dll", "GetModuleHandleW") => {
            if args[0] == 0 {
                ret = state.exe_base;
            } else {
                let wide = name.ends_with('W');
                let n = read_str(mem, args[0], wide);
                let mut base = n.to_ascii_lowercase();
                if !base.ends_with(".dll") {
                    base.push_str(".dll");
                }
                note = format!("module \"{n}\"");
                ret = state.module_handle_for(&base);
            }
        }
        ("kernel32.dll", "LoadLibraryA") | ("kernel32.dll", "LoadLibraryW") => {
            let wide = name.ends_with('W');
            let n = read_str(mem, args[0], wide);
            let base = n.to_ascii_lowercase();
            note = format!("\"{n}\" (fake handle; guest DLL loading is M2+ scope)");
            ret = state.module_handle_for(&base);
        }
        ("kernel32.dll", "GetProcAddress") => {
            let hmod = args[0];
            let dll_name = state
                .handle_to_dll
                .get(&hmod)
                .cloned()
                .unwrap_or_else(|| "unknown.dll".to_string());
            if args[1] & 0xffff_0000 == 0 {
                let ord = (args[1] & 0xffff) as u16;
                note = format!("ordinal {ord} from {dll_name}");
                ret = api.register(&dll_name, None, Some(ord));
            } else {
                let n = mem.read_cstr(args[1], 256).map_err(|e| e.to_string())?;
                note = format!("\"{n}\" from {dll_name}");
                ret = api.register(&dll_name, Some(n.as_str()), None);
            }
        }
        ("kernel32.dll", "FreeLibrary") => {
            ret = 1;
            note = "no-op".into();
        }
        ("kernel32.dll", "GetStdHandle") => {
            ret = 0;
            note = "fake null handle".into();
        }
        ("kernel32.dll", "GetProcessHeap") => ret = state.process_heap,
        ("kernel32.dll", "HeapAlloc") => {
            let p = state.scratch_alloc(mem, args[2].max(1), 16);
            if args[1] & 0x8 != 0 {
                // HEAP_ZERO_MEMORY
                let z = vec![0u8; (args[2] as usize).min(1 << 26)];
                mem.write_bytes(p, &z).map_err(|e| e.to_string())?;
            }
            note = "bump alloc (no real heap yet)".into();
            ret = p;
        }
        ("kernel32.dll", "HeapReAlloc") => {
            ret = state.scratch_alloc(mem, args[3].max(1), 16);
            note = "bump alloc (contents not preserved)".into();
        }
        ("kernel32.dll", "HeapFree") | ("kernel32.dll", "HeapDestroy") => {
            ret = 1;
            note = "no-op".into();
        }
        ("kernel32.dll", "HeapCreate") => {
            ret = 0x7006_0000;
            note = "fake heap handle".into();
        }
        ("kernel32.dll", "HeapSize") => {
            ret = 0x1000;
            note = "fake size".into();
        }
        ("kernel32.dll", "GetProcessHeaps") => {
            ret = 1;
            note = "fake: single heap".into();
            if args[0] >= 1 {
                mem.write_u32(args[1], state.process_heap).ok();
            }
        }
        ("kernel32.dll", "EncodePointer") | ("kernel32.dll", "DecodePointer") => {
            ret = args[0];
            note = "identity encoding".into();
        }
        ("kernel32.dll", "GetModuleFileNameA") | ("kernel32.dll", "GetModuleFileNameW") => {
            let wide = name.ends_with('W');
            let (buf, bufsize) = (args[1], args[2]);
            let path = state.exe_path.clone();
            if args[0] == 0 || args[0] == state.exe_base {
                if wide {
                    let w: Vec<u16> = path.encode_utf16().collect();
                    let n = (w.len() as u32).min(bufsize.saturating_sub(1));
                    for i in 0..n {
                        mem.write_u16(buf + 2 * i, w[i as usize]).map_err(|e| e.to_string())?;
                    }
                    if bufsize > 0 {
                        mem.write_u16(buf + 2 * n, 0).map_err(|e| e.to_string())?;
                    }
                    ret = n;
                } else {
                    let b = path.as_bytes();
                    let n = (b.len() as u32).min(bufsize.saturating_sub(1));
                    mem.write_bytes(buf, &b[..n as usize]).map_err(|e| e.to_string())?;
                    if bufsize > 0 {
                        mem.write_u8(buf + n, 0).map_err(|e| e.to_string())?;
                    }
                    ret = n;
                }
                note = format!("\"{path}\"");
            } else {
                ret = 0;
                note = "module handle not the exe".into();
            }
        }
        ("kernel32.dll", "GetFileType") => {
            ret = 0; // FILE_TYPE_UNKNOWN
        }
        ("kernel32.dll", "GetEnvironmentStrings") => {
            // empty block: "\0\0"
            let p = state.scratch_alloc(mem, 16, 16);
            mem.write_u16(p, 0).map_err(|e| e.to_string())?;
            ret = p;
            note = "empty environment block".into();
        }
        ("kernel32.dll", "GetEnvironmentStringsW") => {
            // empty block: L"\0\0"
            let p = state.scratch_alloc(mem, 16, 16);
            mem.write_u32(p, 0).map_err(|e| e.to_string())?;
            ret = p;
            note = "empty environment block".into();
        }
        ("kernel32.dll", "FreeEnvironmentStringsA") | ("kernel32.dll", "FreeEnvironmentStringsW") => {
            ret = 1;
        }
        ("kernel32.dll", "GetEnvironmentVariableA") | ("kernel32.dll", "GetEnvironmentVariableW") => {
            ret = 0; // not found; GetLastError already 0
            note = "empty environment".into();
        }
        ("kernel32.dll", "SetUnhandledExceptionFilter") => {
            ret = 0;
            note = "no-op".into();
        }
        // --- critical sections: real owner/recursion bookkeeping in the guest
        // CRITICAL_SECTION struct; contention blocks cooperatively. The
        // LockCount field only distinguishes free (-1) vs held (0); waiter
        // counts are not maintained (documented simplification).
        ("kernel32.dll", "InitializeCriticalSection") => {
            let cs = args[0];
            mem.write_u32(cs, 0).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 4, u32::MAX).map_err(|e| e.to_string())?; // LockCount = -1
            mem.write_u32(cs + 8, 0).map_err(|e| e.to_string())?; // RecursionCount
            mem.write_u32(cs + 12, 0).map_err(|e| e.to_string())?; // OwningThread
            mem.write_u32(cs + 16, 0).map_err(|e| e.to_string())?; // LockSemaphore
            mem.write_u32(cs + 20, 0).map_err(|e| e.to_string())?; // SpinCount
            ret = 0; // void
        }
        ("kernel32.dll", "InitializeCriticalSectionAndSpinCount")
        | ("kernel32.dll", "InitializeCriticalSectionEx") => {
            let cs = args[0];
            let spin = if name.ends_with("Ex") { args[2] } else { args[1] };
            mem.write_u32(cs, 0).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 4, u32::MAX).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 8, 0).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 12, 0).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 16, 0).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 20, spin).map_err(|e| e.to_string())?;
            ret = 1;
        }
        ("kernel32.dll", "EnterCriticalSection") => {
            let cs = args[0];
            let me = sched.current_tid();
            let owner = mem.read_u32(cs + 12).map_err(|e| e.to_string())?;
            if owner == 0 || owner == me {
                let rc = mem.read_u32(cs + 8).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 12, me).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 8, rc + 1).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 4, 0).map_err(|e| e.to_string())?;
                ret = 0; // void
            } else {
                sched.take_wake(); // discard Retest; fall through to re-test
                log.note(&format!(
                    "thread {} blocks on CRITICAL_SECTION {cs:#x} (owner tid {owner})",
                    sched.current_tid()
                ));
                sched.block_current(cpu, WaitTarget::CriticalSection(cs), None)?;
                return Ok(());
            }
        }
        ("kernel32.dll", "LeaveCriticalSection") => {
            let cs = args[0];
            let rc = mem
                .read_u32(cs + 8)
                .map_err(|e| e.to_string())?
                .saturating_sub(1);
            mem.write_u32(cs + 8, rc).map_err(|e| e.to_string())?;
            if rc == 0 {
                mem.write_u32(cs + 12, 0).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 4, u32::MAX).map_err(|e| e.to_string())?;
                for i in 0..sched.threads.len() {
                    if matches!(
                        &sched.threads[i].state,
                        ThreadState::Waiting {
                            target: WaitTarget::CriticalSection(a),
                            ..
                        } if *a == cs
                    ) {
                        sched.wake_thread(i, Wake::Retest);
                        break;
                    }
                }
            }
            ret = 0; // void
        }
        ("kernel32.dll", "TryEnterCriticalSection") => {
            let cs = args[0];
            let me = sched.current_tid();
            let owner = mem.read_u32(cs + 12).map_err(|e| e.to_string())?;
            if owner == 0 || owner == me {
                let rc = mem.read_u32(cs + 8).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 12, me).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 8, rc + 1).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 4, 0).map_err(|e| e.to_string())?;
                ret = 1;
            } else {
                ret = 0;
            }
        }
        ("kernel32.dll", "DeleteCriticalSection") => {
            let cs = args[0];
            mem.write_u32(cs + 4, u32::MAX).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 12, 0).map_err(|e| e.to_string())?;
            ret = 0; // void
        }
        // --- TLS / FLS ---
        ("kernel32.dll", "TlsAlloc") => {
            ret = state.next_tls;
            state.next_tls += 1;
        }
        ("kernel32.dll", "TlsSetValue") => {
            state.tls.insert(args[0], args[1]);
            ret = 1;
        }
        ("kernel32.dll", "TlsGetValue") => ret = state.tls.get(&args[0]).copied().unwrap_or(0),
        ("kernel32.dll", "TlsFree") => {
            state.tls.remove(&args[0]);
            ret = 1;
        }
        ("kernel32.dll", "FlsAlloc") => {
            ret = state.next_fls;
            state.next_fls += 1;
            note = "callback not invoked".into();
        }
        ("kernel32.dll", "FlsSetValue") => {
            state.fls.insert(args[0], args[1]);
            ret = 1;
        }
        ("kernel32.dll", "FlsGetValue") => ret = state.fls.get(&args[0]).copied().unwrap_or(0),
        ("kernel32.dll", "FlsFree") => {
            state.fls.remove(&args[0]);
            ret = 1;
        }
        ("kernel32.dll", "LoadLibraryExA") | ("kernel32.dll", "LoadLibraryExW") => {
            let wide = name.ends_with('W');
            let n = read_str(mem, args[0], wide);
            let base = n.to_ascii_lowercase();
            note = format!("\"{n}\" flags={:#x} (fake handle)", args[2]);
            ret = state.module_handle_for(&base);
        }
        ("kernel32.dll", "GetModuleHandleExA") | ("kernel32.dll", "GetModuleHandleExW") => {
            let wide = name.ends_with('W');
            let n = read_str(mem, args[1], wide);
            let mut base = n.to_ascii_lowercase();
            if !base.is_empty() && !base.ends_with(".dll") {
                base.push_str(".dll");
            }
            let h = if base.is_empty() {
                state.exe_base
            } else {
                state.module_handle_for(&base)
            };
            mem.write_u32(args[2], h).map_err(|e| e.to_string())?;
            note = format!("module \"{n}\"");
            ret = 1;
        }
        ("kernel32.dll", "GetCurrentPackageId") => {
            ret = 0x3d54; // APPMODEL_ERROR_NO_PACKAGE (desktop app)
            note = "not a packaged app".into();
        }
        ("kernel32.dll", "InterlockedIncrement") => {
            let v = mem.read_u32(args[0]).map_err(|e| e.to_string())?.wrapping_add(1);
            mem.write_u32(args[0], v).map_err(|e| e.to_string())?;
            ret = v;
        }
        ("kernel32.dll", "InterlockedDecrement") => {
            let v = mem.read_u32(args[0]).map_err(|e| e.to_string())?.wrapping_sub(1);
            mem.write_u32(args[0], v).map_err(|e| e.to_string())?;
            ret = v;
        }
        ("kernel32.dll", "InterlockedExchange") => {
            let old = mem.read_u32(args[0]).map_err(|e| e.to_string())?;
            mem.write_u32(args[0], args[1]).map_err(|e| e.to_string())?;
            ret = old;
        }
        ("kernel32.dll", "InterlockedCompareExchange") => {
            let old = mem.read_u32(args[0]).map_err(|e| e.to_string())?;
            if old == args[1] {
                mem.write_u32(args[0], args[2]).map_err(|e| e.to_string())?;
            }
            ret = old;
        }
        ("user32.dll", "GetProcessWindowStation") => {
            ret = 0x7007_0000;
            note = "fake winsta handle".into();
        }
        ("user32.dll", "GetUserObjectInformationW") => {
            ret = 0;
            note = "fake: unsupported winsta query".into();
        }
        ("user32.dll", "MessageBoxA") | ("user32.dll", "MessageBoxW") => {
            let wide = name.ends_with('W');
            let text = read_str(mem, args[1], wide);
            let caption = read_str(mem, args[2], wide);
            note = format!("SHOWN: [{caption}] {text}");
            ret = 1; // IDOK
        }
        ("user32.dll", "GetActiveWindow") | ("user32.dll", "GetLastActivePopup") => ret = 0,
        ("mscoree.dll", "CorExitProcess") => {
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            note = "CLR-style exit".into();
            ret = 0;
        }
        _ => {
            match crt_dispatch(dll, name, &args, cpu, mem, api, state, log, sched, max_instr)? {
                Some(v) => {
                    ret = v;
                    note = "crt shim".into();
                }
                None => {
                    implemented = false;
                    ret = 0;
                    if stack_args_bytes(dll, name).is_none() {
                        note = "unknown stdcall arity, assuming cdecl/0-byte cleanup".into();
                    }
                }
            }
        }
    }

    let stack_args = stack_args_bytes(dll, name).unwrap_or(0);
    let nargs = (stack_args / 4).min(6) as usize;
    log.log_call(dll, name, &args[..nargs.max(1)], ret, implemented, &note);

    cpu.regs[0] = ret; // eax
    let retaddr = mem
        .read_u32(esp)
        .map_err(|e| format!("stub stack fault: {e}"))?;
    cpu.set_esp(esp.wrapping_add(4 + stack_args));
    cpu.eip = retaddr;
    Ok(())
}

/// Minimal CRT (UCRT/msvcr) shims. All cdecl. Ok(None) = not covered (caller
/// logs it as unimplemented).
#[allow(clippy::too_many_arguments)]
fn crt_dispatch(
    dll: &str,
    name: &str,
    args: &[u32; 6],
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<Option<u32>, String> {
    if !(dll.starts_with("api-ms-win-crt-")
        || dll == "ucrtbase.dll"
        || dll == "msvcrt.dll"
        || (dll.starts_with("msvcr") && dll.ends_with(".dll")))
    {
        return Ok(None);
    }
    let r = match name {
        "__acrt_iob_func" => state
            .scratch_alloc(mem, 0x100, 16)
            .wrapping_add(args[0].wrapping_mul(0x40)),
        "__p__fmode" | "__p__commode" | "__p___argc" | "__p__timezone" | "__p__daylight" => {
            let p = state.scratch_alloc(mem, 16, 16);
            if name == "__p___argc" {
                mem.write_u32(p, 1).map_err(|e| e.to_string())?;
            }
            p
        }
        "__p__acmdln" => {
            let p = state.scratch_alloc(mem, 4, 4);
            mem.write_u32(p, state.cmdline_a).map_err(|e| e.to_string())?;
            p
        }
        "__p__wcmdln" => {
            let p = state.scratch_alloc(mem, 4, 4);
            mem.write_u32(p, state.cmdline_w).map_err(|e| e.to_string())?;
            p
        }
        "__p___argv" | "__p___wargv" => {
            let p = state.scratch_alloc(mem, 4, 4);
            let (_, argv, _) = state.build_argv(mem);
            mem.write_u32(p, argv).map_err(|e| e.to_string())?;
            p
        }
        "__p__environ" | "__p__wenviron" | "__p___initenv" | "__p___winitenv" => {
            let p = state.scratch_alloc(mem, 4, 4);
            let (_, _, envp) = state.build_argv(mem);
            mem.write_u32(p, envp).map_err(|e| e.to_string())?;
            p
        }
        "__p__pgmptr" | "__p__wpgmptr" => {
            let p = state.scratch_alloc(mem, 4, 4);
            mem.write_u32(p, state.cmdline_a).map_err(|e| e.to_string())?;
            p
        }
        "__getmainargs" => {
            let (argc, argv, envp) = state.build_argv(mem);
            mem.write_u32(args[0], argc).map_err(|e| e.to_string())?;
            mem.write_u32(args[1], argv).map_err(|e| e.to_string())?;
            mem.write_u32(args[2], envp).map_err(|e| e.to_string())?;
            0
        }
        "_initterm" | "_initterm_e" => {
            // Walk an array of guest function pointers and call each
            // non-null initializer (C++ static ctors / C initializers).
            let mut a = args[0];
            let end = args[1];
            let mut first_err = 0u32;
            while a < end {
                let f = mem.read_u32(a).unwrap_or(0);
                if f != 0 {
                    let v = cpu.call_guest(mem, api, state, log, sched, f, &[], max_instr)?;
                    if name.ends_with("_e") && v != 0 && first_err == 0 {
                        first_err = v;
                    }
                }
                a = a.wrapping_add(4);
            }
            first_err
        }
        "_initialize_narrow_environment" | "_initialize_wide_environment"
        | "_configure_narrow_argv" | "_configure_wide_argv" | "_initialize_onexit_table"
        | "_register_onexit_function" | "_crt_atexit" | "_crt_at_quick_exit"
        | "_set_invalid_parameter_handler" | "_set_app_type" | "_set_fmode"
        | "_set_new_mode" | "setvbuf" | "_crt_debugger_hook" | "_controlfp_s" | "_lock"
        | "_unlock" | "__setusermatherr" | "_register_thread_local_exe_atexit_callback" => 0,
        "malloc" => state.scratch_alloc(mem, args[0].max(1), 16),
        "calloc" => {
            let n = args[0].max(1).saturating_mul(args[1].max(1));
            let p = state.scratch_alloc(mem, n, 16);
            let z = vec![0u8; n.min(1 << 26) as usize];
            mem.write_bytes(p, &z).map_err(|e| e.to_string())?;
            p
        }
        "realloc" => state.scratch_alloc(mem, args[1].max(1), 16),
        "free" | "_free_dbg" | "_aligned_free" => 0,
        "memcpy" | "memmove" => {
            let n = (args[2] as usize).min(1 << 26);
            let mut tmp = vec![0u8; n];
            mem.read_bytes(args[1], &mut tmp).map_err(|e| e.to_string())?;
            mem.write_bytes(args[0], &tmp).map_err(|e| e.to_string())?;
            args[0]
        }
        "memset" => {
            let n = (args[2] as usize).min(1 << 26);
            let tmp = vec![args[1] as u8; n];
            mem.write_bytes(args[0], &tmp).map_err(|e| e.to_string())?;
            args[0]
        }
        "strlen" => mem
            .read_cstr(args[0], 1 << 20)
            .map(|s| s.len() as u32)
            .unwrap_or(0),
        "exit" | "_exit" | "_cexit" | "quick_exit" => {
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            0
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}
