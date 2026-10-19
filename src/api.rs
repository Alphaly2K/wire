//! Win32 API stub layer.
//!
//! Every imported function gets a unique guest address in a reserved stub
//! range. When the interpreter's EIP enters that range, control transfers to
//! `dispatch` on the host: the call is logged (name, args, return value), a
//! fake or real result is placed in EAX, the stdcall stack is cleaned up, and
//! execution resumes at the return address. Unimplemented APIs are not silent:
//! they are counted and reported, which is the primary M0 deliverable.

use crate::cpu::{Cpu, GuestCallOutcome, EDX};
use crate::mem::GuestMem;
use crate::thread::{
    target_mentions, Sched, ThreadState, WaitTarget, Wake, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use std::collections::HashMap;
pub type Args = [u32; 16];
use std::fs::File;
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub use crate::runtime::api_log::ApiLogger;
pub use crate::x86::address_space::{STUB_BASE, STUB_CAPACITY, STUB_STRIDE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiEntry {
    pub dll: String,  // lowercased, includes ".dll" (or pseudo-module "wire")
    pub name: String, // export name, or "#<ordinal>"
    pub ordinal: Option<u16>,
}

pub struct ApiTable {
    pub entries: Vec<ApiEntry>,
    bindings: Vec<Arc<BoundEntry>>,
    by_key: HashMap<(String, String), u32>,
    data_exports: HashMap<(String, String), u32>,
    next_data: u32,
}

/// Immutable call metadata. The public entry view remains compatible; edits to
/// that view invalidate this snapshot before the next call (including reentry).
struct BoundEntry {
    entry: ApiEntry,
    typed: Option<crate::win32::catalog::ApiId>,
    stack_bytes: Option<u32>,
    unimplemented: bool,
}
impl BoundEntry {
    fn new(entry: ApiEntry) -> Self {
        use crate::win32::catalog::{resolve, Architecture, ExecutionPolicy, Resolution};
        let contract = match resolve(Architecture::X86, &entry.dll, &entry.name) {
            Resolution::Bound(contract) => Some(contract),
            Resolution::WrongDll(_) | Resolution::Legacy => None,
        };
        let typed = contract
            .filter(|c| c.execution(Architecture::X86) == ExecutionPolicy::Typed)
            .map(|c| c.id);
        let unimplemented = contract.is_some_and(|c| {
            c.execution(Architecture::X86) == ExecutionPolicy::LegacyUnimplemented
        });
        let stack_bytes = contract.map_or_else(
            || legacy_stack_args_bytes(&entry.dll, &entry.name),
            |c| Some(c.stdcall_bytes()),
        );
        Self {
            entry,
            typed,
            stack_bytes,
            unimplemented,
        }
    }
}

impl Default for ApiTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiTable {
    pub fn new() -> ApiTable {
        let mut t = ApiTable {
            entries: Vec::new(),
            bindings: Vec::new(),
            by_key: HashMap::new(),
            data_exports: HashMap::new(),
            next_data: 0,
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
            .or_else(|| ordinal.and_then(|o| crate::win32_abi::ordinal_name(&dll, o)))
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("#{}", ordinal.unwrap_or(0)));
        let key = (dll.clone(), name.clone());
        if let Some(&addr) = self.data_exports.get(&key) {
            return addr;
        }
        if let Some(&addr) = self.by_key.get(&key) {
            if ordinal.is_some() {
                let index = ((addr - STUB_BASE) / STUB_STRIDE) as usize;
                self.entries[index].ordinal = ordinal;
            }
            return addr;
        }
        let idx = self.entries.len();
        assert!(idx < STUB_CAPACITY, "API stub table overflow");
        let addr = STUB_BASE + idx as u32 * STUB_STRIDE;
        let entry = ApiEntry { dll, name, ordinal };
        self.bindings.push(Arc::new(BoundEntry::new(entry.clone())));
        self.entries.push(entry);
        self.by_key.insert(key, addr);
        addr
    }

    fn binding(&mut self, index: usize) -> Arc<BoundEntry> {
        // Legacy callers can mutate/append/reorder the public entries vector.
        // Resolve only on a changed identity, never on an unchanged call.
        while self.bindings.len() <= index {
            let next = self.bindings.len();
            self.bindings
                .push(Arc::new(BoundEntry::new(self.entries[next].clone())));
        }
        if self.bindings[index].entry != self.entries[index] {
            self.bindings[index] = Arc::new(BoundEntry::new(self.entries[index].clone()));
        }
        self.bindings[index].clone()
    }

    pub fn stub_index(&self, addr: u32) -> Option<usize> {
        if addr >= STUB_BASE && addr < STUB_BASE + (self.entries.len() as u32) * STUB_STRIDE {
            Some(((addr - STUB_BASE) / STUB_STRIDE) as usize)
        } else {
            None
        }
    }

    pub fn lookup(&self, dll: &str, name: &str) -> Option<u32> {
        let key = (dll.to_ascii_lowercase(), name.to_string());
        self.data_exports
            .get(&key)
            .or_else(|| self.by_key.get(&key))
            .copied()
    }
    pub fn imported_dlls(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .map(|e| e.dll.as_str())
            .chain(self.data_exports.keys().map(|(dll, _)| dll.as_str()))
    }

    /// CRT exports such as _acmdln are guest variables, never callable stubs.
    /// Imports, GetProcAddress and __p__ accessors share each module's stable
    /// writable storage. Reserve a checked runtime arena separate from scratch.
    pub(crate) fn data_export(
        &mut self,
        mem: &mut GuestMem,
        dll: &str,
        name: &str,
        cmdline_a: u32,
        cmdline_w: u32,
    ) -> Result<Option<u32>, String> {
        let dll = dll.to_ascii_lowercase();
        if !(dll == "msvcrt.dll"
            || dll == "ucrtbase.dll"
            || dll.starts_with("api-ms-win-crt-")
            || (dll.starts_with("msvcr") && dll.ends_with(".dll")))
        {
            return Ok(None);
        }
        let version = crate::runtime::environment::GuestEnvironment::pe32().version();
        let initial = match name {
            "_acmdln" => cmdline_a,
            "_wcmdln" => cmdline_w,
            "_fmode" | "_commode" | "_adjust_fdiv" | "_timezone" | "_daylight" => 0,
            "__argc" => 1,
            "_winmajor" => version.major,
            "_winminor" => version.minor,
            "_winver" => (version.major << 8) | version.minor,
            "_osver" => version.build,
            _ => return Ok(None),
        };
        let key = (dll, name.to_string());
        if let Some(&addr) = self.data_exports.get(&key) {
            return Ok(Some(addr));
        }
        const DATA_BASE: u32 = 0x00ec_0000;
        if self.next_data == 0 {
            mem.map_runtime(DATA_BASE, 0x10000)
                .map_err(|e| format!("CRT data arena: Win32 error {e}"))?;
            self.next_data = DATA_BASE;
        }
        if self.next_data >= DATA_BASE + 0x10000 {
            return Err("CRT data arena exhausted".into());
        }
        let addr = self.next_data;
        mem.write_u32(addr, initial).map_err(|e| e.to_string())?;
        self.next_data += 4;
        self.data_exports.insert(key, addr);
        Ok(Some(addr))
    }
}

/// Guest scratch space for fake CRT pointers (starts here, bump-allocated).
pub const SCRATCH_BASE: u32 = 0x00f0_0000;

/// Kernel objects behind guest HANDLE values (M2). Files join this enum when
/// real file I/O lands.
#[derive(Debug)]
pub enum HandleObj {
    File {
        file: File,
        readable: bool,
        writable: bool,
    },
    Search {
        entries: Vec<std::path::PathBuf>,
        next: usize,
    },
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
    Process {
        access: u32,
    },
}

/// Mutable host-side state for the emulated process.
pub struct ApiState {
    pub(crate) environment: crate::runtime::environment::GuestEnvironment,
    pub last_error: u32,
    pub unhandled_filter: u32,
    pub start: Instant,
    pub exe_base: u32,
    pub exe_path: String,
    pub cmdline_a: u32,
    pub cmdline_w: u32,
    pub scratch_top: u32,
    pub argv_ptr: u32,
    pub envp_ptr: u32,
    pub crt_app_type: u32,
    crt_errno: HashMap<u32, u32>,
    pub process_heap: u32,
    pub heap: crate::heap::GuestHeap,
    pub fs: Option<crate::fs::GuestFs>,
    pub(crate) locals: crate::win32::kernel::locals32::LocalSlots32,
    pub handles: HashMap<u32, HandleObj>,
    pub named_objs: HashMap<String, u32>, // event/mutex/semaphore name -> handle
    pub next_handle: u32,
    pub user32: crate::user32::User32,
    pub(crate) menus: crate::menu::Menus,
    pub modules: crate::modules::Modules,
    pub gdiplus: crate::gdiplus::GdiPlus,
    pub graphics: crate::graphics::Graphics,
    pub d3d9: crate::d3d9::D3D9,
    pub dsound: crate::dsound::DSound,
    pub(crate) wmi: crate::wmi::Wmi,
    pub presenter: crate::host::ui::Presenter,
    pub stop_after_presents: Option<u64>,
    pub(crate) bstrs: crate::win32::com::bstr::Bstrings32,
    pub(crate) apartments: crate::win32::com::apartment::Apartments32,
}

impl ApiState {
    pub fn new(exe_base: u32, exe_path: String, cmdline_a: u32, cmdline_w: u32) -> ApiState {
        let mut s = ApiState {
            environment: crate::runtime::environment::GuestEnvironment::pe32(),
            last_error: 0,
            unhandled_filter: 0,
            start: Instant::now(),
            exe_base,
            exe_path,
            cmdline_a,
            cmdline_w,
            scratch_top: SCRATCH_BASE,
            argv_ptr: 0,
            envp_ptr: 0,
            crt_app_type: 1,
            crt_errno: HashMap::new(),
            process_heap: 0x7005_0000,
            heap: crate::heap::GuestHeap::new(0x7005_0000),
            fs: None,
            locals: crate::win32::kernel::locals32::LocalSlots32::default(),
            handles: HashMap::new(),
            named_objs: HashMap::new(),
            next_handle: 0x100,
            user32: crate::user32::User32::default(),
            menus: crate::menu::Menus::default(),
            modules: crate::modules::Modules::default(),
            gdiplus: crate::gdiplus::GdiPlus::default(),
            graphics: crate::graphics::Graphics::default(),
            d3d9: crate::d3d9::D3D9::default(),
            dsound: crate::dsound::DSound::default(),
            wmi: crate::wmi::Wmi::default(),
            presenter: crate::host::ui::Presenter::default(),
            stop_after_presents: None,
            bstrs: Default::default(),
            apartments: Default::default(),
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
        self.modules.provider_handle_for(dll)
    }

    /// Bump-allocate guest scratch memory (for fake CRT structures).
    pub fn scratch_alloc(&mut self, mem: &mut GuestMem, size: u32, align: u32) -> u32 {
        let allocation = (|| {
            if !align.is_power_of_two() {
                return None;
            }
            let a = self.scratch_top.checked_add(align - 1)? & !(align - 1);
            let end = a.checked_add(size.max(1))?;
            if end > 0x01000000 || mem.commit(a, size.max(1)).is_err() {
                return None;
            }
            Some((a, end))
        })();
        let Some((a, end)) = allocation else {
            self.last_error = 8;
            return 0;
        };
        self.scratch_top = end;
        a
    }
    fn errno_address(&mut self, mem: &mut GuestMem, tid: u32) -> Result<u32, String> {
        if let Some(&p) = self.crt_errno.get(&tid) {
            return Ok(p);
        }
        let p = self.scratch_alloc(mem, 4, 4);
        if p == 0 {
            return Err("CRT errno allocation failed".into());
        }
        mem.write_u32(p, 0).map_err(|e| e.to_string())?;
        self.crt_errno.insert(tid, p);
        Ok(p)
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

pub(crate) fn read_str(mem: &GuestMem, addr: u32, wide: bool) -> String {
    if addr == 0 {
        return String::new();
    }
    if wide {
        mem.read_wstr(addr, 260)
            .unwrap_or_else(|_| "<fault>".into())
    } else {
        crate::file_api::read_name(mem, addr, false).unwrap_or_else(|_| "<fault>".into())
    }
}

/// Native exports with a known ABI, including supported cdecl CRT helpers.
/// Known export ABI for diagnostics and dynamic lookup. Registered ABI alone
/// does not establish full implementation; consult runtime unimplemented logs.
pub fn native_export(dll: &str, name: &str) -> bool {
    stack_args_bytes(dll, name).is_some()
        || (dll == "msvcrt.dll"
            && matches!(
                name,
                "malloc"
                    | "calloc"
                    | "realloc"
                    | "free"
                    | "memcpy"
                    | "memmove"
                    | "memset"
                    | "memcmp"
                    | "strlen"
                    | "strcmp"
                    | "strncmp"
                    | "strcpy"
                    | "strncpy"
                    | "__getmainargs"
                    | "__wgetmainargs"
                    | "_initterm"
                    | "_initterm_e"
                    | "_onexit"
                    | "atexit"
                    | "exit"
                    | "_exit"
                    | "abort"
            ))
}

/// stdcall stack cleanup in bytes. Unknown functions retain the existing
/// cdecl/zero-cleanup diagnostic path.
fn stack_args_bytes(dll: &str, name: &str) -> Option<u32> {
    use crate::win32::catalog::{resolve, Architecture, Resolution};
    if let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) {
        return Some(contract.stdcall_bytes());
    }
    legacy_stack_args_bytes(dll, name)
}

fn legacy_stack_args_bytes(dll: &str, name: &str) -> Option<u32> {
    if dll == "dsound.dll" && name.contains('.') {
        return crate::dsound::stack_bytes(name);
    }
    if dll == "wire.wmi" {
        return crate::wmi::stack_bytes(name);
    }
    if dll == "d3d9.dll" {
        return crate::d3d9::stack_bytes(name);
    }
    if dll == "gdiplus.dll" {
        return crate::gdiplus::stack_bytes(name);
    }
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
                ("SetHandleCount", 4),
                ("GetTempPathA", 8),
                ("GetTempPathW", 8),
                ("ExitProcess", 4),
                ("TerminateProcess", 8),
                ("SetUnhandledExceptionFilter", 4),
                ("UnhandledExceptionFilter", 4),
                ("DebugBreak", 0),
                ("OutputDebugStringA", 4),
                ("OutputDebugStringW", 4),
                ("FormatMessageA", 28),
                ("FormatMessageW", 28),
                ("TerminateThread", 8),
                ("GetDiskFreeSpaceA", 20),
                ("GetDiskFreeSpaceW", 20),
                ("GetDriveTypeA", 4),
                ("GetDriveTypeW", 4),
                ("DeviceIoControl", 32),
                ("GetOverlappedResult", 16),
                ("SetConsoleCtrlHandler", 8),
                ("GetProcessTimes", 20),
                ("GetProcessWorkingSetSize", 12),
                ("EncodePointer", 4),
                ("DecodePointer", 4),
                ("IsWow64Process", 8),
                ("GetLogicalProcessorInformation", 8),
                ("RaiseException", 16),
                ("RtlUnwind", 16),
                ("GetCurrentPackageId", 8),
                ("CreateThreadpoolTimer", 12),
                ("SetThreadpoolTimer", 16),
                ("WaitForThreadpoolTimerCallbacks", 8),
                ("CloseThreadpoolTimer", 4),
                ("CreateThreadpoolWait", 12),
                ("SetThreadpoolWait", 12),
                ("CloseThreadpoolWait", 4),
                ("FlushProcessWriteBuffers", 0),
                ("FreeLibraryWhenCallbackReturns", 8),
                ("GetCurrentProcessorNumber", 0),
                ("SetThreadStackGuarantee", 4),
                ("SetDefaultDllDirectories", 4),
                ("GlobalAddAtomW", 4),
                ("GlobalDeleteAtom", 4),
                ("FreeConsole", 0),
                ("AttachConsole", 4),
                ("GetConsoleMode", 8),
                ("WriteConsoleA", 20),
                ("WriteConsoleW", 20),
                ("SetConsoleTitleW", 4),
                ("GetConsoleTitleW", 8),
                ("GetConsoleProcessList", 8),
                ("ReadConsoleW", 20),
                ("AreFileApisANSI", 0),
                ("GetVolumeInformationW", 32),
                ("SearchPathW", 24),
                ("GetSystemDirectoryW", 8),
                ("GetWindowsDirectoryW", 8),
                ("CreateProcessW", 40),
                ("CreatePipe", 16),
                ("GetTimeFormatW", 24),
                ("GetDateFormatW", 24),
                ("GetLargePageMinimum", 0),
            ],
        );
        add(
            "user32.dll",
            &[
                ("GetProcessWindowStation", 0),
                ("GetUserObjectInformationW", 20),
                ("GetUserObjectInformationA", 20),
                ("MessageBoxA", 16),
                ("MessageBoxW", 16),
                ("MessageBoxExA", 20),
                ("MessageBoxExW", 20),
                ("BringWindowToTop", 4),
                ("WindowFromPoint", 8),
                ("LoadAcceleratorsA", 8),
                ("LoadAcceleratorsW", 8),
                ("TranslateAcceleratorA", 12),
                ("TranslateAcceleratorW", 12),
                ("GetShellWindow", 0),
                ("DrawIcon", 16),
                ("FillRect", 12),
                ("FrameRect", 12),
                ("InvertRect", 8),
                ("SetKeyboardState", 4),
                ("VkKeyScanA", 4),
                ("VkKeyScanW", 4),
                ("RegisterHotKey", 16),
                ("UnregisterHotKey", 8),
                ("CharNextA", 4),
                ("CharNextW", 4),
                ("CharPrevA", 8),
                ("CharPrevW", 8),
                ("CharUpperA", 4),
                ("CharUpperW", 4),
                ("CharLowerA", 4),
                ("CharLowerW", 4),
                ("wsprintfA", 0),
                ("wsprintfW", 0), // cdecl varargs
                ("wvsprintfA", 12),
                ("wvsprintfW", 12),
                ("DrawTextA", 20),
                ("DrawTextW", 20),
                ("DrawTextExA", 24),
                ("DrawTextExW", 24),
                ("DialogBoxParamA", 20),
                ("DialogBoxParamW", 20),
                ("CreateDialogParamA", 20),
                ("CreateDialogParamW", 20),
                ("EndDialog", 8),
                ("GetSysColor", 4),
                ("GetSysColorBrush", 4),
                ("AttachThreadInput", 12),
                ("ChangeDisplaySettingsA", 8),
                ("ChangeDisplaySettingsW", 8),
                ("SetWindowsHookExA", 16),
                ("SetWindowsHookExW", 16),
                ("UnhookWindowsHookEx", 4),
                ("CallNextHookEx", 16),
                ("ActivateKeyboardLayout", 8),
                ("ToAscii", 20),
            ],
        );
        add(
            "gdi32.dll",
            &[
                ("CreateDCA", 16),
                ("CreateDCW", 16),
                ("CreateBitmap", 20),
                ("CreateBitmapIndirect", 4),
                ("StretchBlt", 44),
                ("PatBlt", 24),
                ("MaskBlt", 48),
                ("TransparentBlt", 44),
                ("AlphaBlend", 44),
                ("GradientFill", 24),
                ("SetDIBits", 28),
                ("SetDIBitsToDevice", 48),
                ("CreatePen", 12),
                ("CreatePenIndirect", 4),
                ("CreateSolidBrush", 4),
                ("CreateHatchBrush", 8),
                ("CreatePatternBrush", 4),
                ("CreateBrushIndirect", 4),
                ("CreateRectRgn", 16),
                ("CreateRectRgnIndirect", 4),
                ("CreateEllipticRgn", 16),
                ("CreateRoundRectRgn", 24),
                ("CombineRgn", 16),
                ("SelectClipRgn", 8),
                ("SetBkMode", 8),
                ("GetBkMode", 4),
                ("TextOutA", 20),
                ("TextOutW", 20),
                ("ExtTextOutA", 32),
                ("ExtTextOutW", 32),
                ("GetTextExtentPoint32A", 16),
                ("GetTextExtentPoint32W", 16),
                ("GetTextExtentExPointA", 28),
                ("GetTextExtentExPointW", 28),
                ("GetCharWidthA", 16),
                ("GetCharWidthW", 16),
                ("GetCharWidth32A", 16),
                ("GetCharWidth32W", 16),
                ("GetTextMetricsA", 8),
                ("GetOutlineTextMetricsA", 12),
                ("GetGlyphOutlineA", 28),
                ("GetGlyphOutlineW", 28),
                ("SetTextAlign", 8),
                ("GetTextAlign", 4),
                ("SetTextCharacterExtra", 8),
                ("Rectangle", 20),
                ("Ellipse", 20),
                ("RoundRect", 28),
                ("Polygon", 12),
                ("Polyline", 12),
                ("MoveToEx", 16),
                ("LineTo", 12),
                ("Arc", 36),
                ("Pie", 36),
                ("SetROP2", 8),
                ("GetROP2", 4),
                ("SetPolyFillMode", 8),
                ("SetViewportExtEx", 16),
                ("SetWindowExtEx", 16),
                ("DPtoLP", 12),
                ("LPtoDP", 12),
                ("GetDCOrgEx", 8),
                ("SelectPalette", 12),
                ("RealizePalette", 4),
                ("CreatePalette", 4),
                ("SetPaletteEntries", 16),
                ("GetPaletteEntries", 16),
                ("SetStretchBltMode", 8),
                ("GetStretchBltMode", 4),
                ("SetBrushOrgEx", 16),
                ("UnrealizeObject", 4),
                ("GetClipBox", 8),
                ("IntersectClipRect", 20),
                ("ExcludeClipRect", 20),
                ("SaveDC", 4),
                ("RestoreDC", 8),
                ("GetTextFaceA", 12),
                ("GetTextFaceW", 12),
                ("EnumFontFamiliesExA", 20),
                ("EnumFontFamiliesExW", 20),
                ("GetFontData", 20),
                ("AddFontResourceA", 4),
                ("AddFontResourceW", 4),
                ("AddFontMemResourceEx", 16),
                ("RemoveFontMemResourceEx", 4),
                ("AddFontResourceExA", 12),
                ("AddFontResourceExW", 12),
                ("RemoveFontResourceExA", 12),
                ("RemoveFontResourceExW", 12),
                ("RemoveFontResourceA", 4),
                ("RemoveFontResourceW", 4),
                ("CreateHalftonePalette", 4),
                ("SetColorAdjustment", 8),
                ("StartDocA", 8),
                ("StartDocW", 8),
                ("EndDoc", 4),
                ("StartPage", 4),
                ("EndPage", 4),
                ("SetPixelFormat", 12),
                ("SwapBuffers", 4),
                ("ChoosePixelFormat", 8),
            ],
        );
        add(
            "advapi32.dll",
            &[
                ("RegDeleteKeyA", 8),
                ("RegDeleteKeyW", 8),
                ("RegEnumKeyExA", 32),
                ("RegEnumValueA", 32),
                ("RegQueryInfoKeyA", 48),
                ("LookupPrivilegeValueA", 12),
                ("AdjustTokenPrivileges", 24),
            ],
        );
        add(
            "version.dll",
            &[
                ("GetFileVersionInfoA", 16),
                ("GetFileVersionInfoW", 16),
                ("GetFileVersionInfoSizeA", 8),
                ("GetFileVersionInfoSizeW", 8),
                ("VerQueryValueA", 16),
                ("VerQueryValueW", 16),
            ],
        );
        add(
            "winmm.dll",
            &[
                ("timeSetEvent", 20),
                ("timeKillEvent", 4),
                ("waveOutGetNumDevs", 0),
                ("waveOutOpen", 24),
                ("waveOutClose", 4),
                ("waveOutPrepareHeader", 12),
                ("waveOutUnprepareHeader", 12),
                ("waveOutWrite", 12),
                ("waveOutReset", 4),
                ("waveOutPause", 4),
                ("waveOutRestart", 4),
                ("waveOutGetPosition", 12),
                ("waveOutSetVolume", 8),
                ("mciSendStringA", 16),
                ("mciSendStringW", 16),
                ("PlaySoundA", 12),
                ("PlaySoundW", 12),
                ("midiOutGetNumDevs", 0),
                ("joyGetNumDevs", 0),
            ],
        );
        add(
            "ole32.dll",
            &[("CreateStreamOnHGlobal", 12), ("GetHGlobalFromStream", 8)],
        );
        add(
            "shell32.dll",
            &[
                ("ShellExecuteA", 24),
                ("ShellExecuteW", 24),
                ("SHGetFolderPathA", 20),
                ("SHGetFolderPathW", 20),
                ("SHGetSpecialFolderPathA", 16),
                ("SHGetSpecialFolderPathW", 16),
                ("SHGetKnownFolderPath", 16),
                ("SHBrowseForFolderA", 4),
                ("SHGetPathFromIDListA", 8),
                ("DragAcceptFiles", 8),
                ("DragQueryFileA", 16),
                ("DragQueryFileW", 16),
                ("DragFinish", 4),
                ("CommandLineToArgvW", 8),
            ],
        );
        add(
            "shlwapi.dll",
            &[
                ("PathCombineA", 12),
                ("PathCombineW", 12),
                ("PathFindExtensionA", 4),
                ("PathFindFileNameA", 4),
                ("PathRemoveExtensionA", 4),
                ("StrStrIA", 8),
                ("StrStrIW", 8),
            ],
        );
        add("ws2_32.dll", &[("closesocket", 4)]);
        add(
            "mpr.dll",
            &[("WNetGetUniversalNameA", 16), ("WNetGetUniversalNameW", 16)],
        );
        add("ntdll.dll", &[("RtlUnwind", 16)]);
        add(
            "user32.dll",
            &[
                ("SetWindowRgn", 12),
                ("DestroyCaret", 0),
                ("OpenClipboard", 4),
                ("SetCaretPos", 8),
                ("GetClipboardData", 4),
                ("IsClipboardFormatAvailable", 4),
                ("SetRectEmpty", 4),
                ("EqualRect", 8),
                ("SetClipboardData", 8),
                ("CloseClipboard", 0),
                ("GetPriorityClipboardFormat", 8),
                ("LoadCursorFromFileW", 4),
                ("DestroyAcceleratorTable", 4),
                ("CreateAcceleratorTableW", 8),
                ("CreateCaret", 16),
            ],
        );
        add(
            "gdi32.dll",
            &[
                ("EnumFontsW", 16),
                ("CreateDIBitmap", 24),
                ("ExtCreateRegion", 12),
                ("GetOutlineTextMetricsW", 12),
            ],
        );
        add(
            "comdlg32.dll",
            &[
                ("GetSaveFileNameW", 4),
                ("GetOpenFileNameW", 4),
                ("CommDlgExtendedError", 0),
            ],
        );
        add(
            "shell32.dll",
            &[("SHBrowseForFolderW", 4), ("SHGetPathFromIDListW", 8)],
        );
        add(
            "ole32.dll",
            &[
                ("CreateItemMoniker", 12),
                ("GetRunningObjectTable", 8),
                ("CoFreeUnusedLibraries", 0),
                ("PropVariantClear", 4),
            ],
        );
        add("oleaut32.dll", &[("#9", 4)]); // VariantClear
        add("dbghelp.dll", &[("MiniDumpWriteDump", 28)]);
        add("quartz.dll", &[("AMGetErrorTextW", 12)]);
        add("propsys.dll", &[("PropVariantToDouble", 8)]);
        add(
            "mfplat.dll",
            &[
                ("MFShutdown", 0),
                ("MFFrameRateToAverageTimePerFrame", 12),
                ("MFStartup", 8),
            ],
        );
        add(
            "mf.dll",
            &[
                ("MFCreateVideoRendererActivate", 8),
                ("MFCreateAudioRendererActivate", 4),
                ("MFCreateTopologyNode", 8),
                ("MFCreateTopology", 4),
                ("MFCreateMediaSession", 8),
                ("MFCreateSourceResolver", 4),
            ],
        );
        add(
            "ddraw.dll",
            &[
                ("DirectDrawCreate", 12),
                ("DirectDrawCreateEx", 16),
                ("DirectDrawEnumerateA", 8),
            ],
        );
        add(
            "dsound.dll",
            &[
                ("DirectSoundCreate", 12),
                ("DirectSoundCreate8", 12),
                ("DirectSoundEnumerateA", 8),
            ],
        );
        add(
            "dinput.dll",
            &[
                ("DirectInputCreateA", 16),
                ("DirectInputCreateW", 16),
                ("DirectInput8Create", 20),
            ],
        );
        m
    });
    if let Some(&b) = t.get(&(dll, name)) {
        return Some(b);
    }
    if let Some(bytes) = crate::win32_abi::stack_bytes(dll, name) {
        return Some(bytes);
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

// --- kernel-object wait helpers (M2) ---

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
        _ => false,
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
        _ => false,
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
pub(crate) fn signal_audio_event(state: &mut ApiState, sched: &mut Sched, h: u32) {
    if let Some(HandleObj::Event { manual, signaled }) = state.handles.get_mut(&h) {
        *signaled = true;
        let n = if *manual { usize::MAX } else { 1 };
        wake_obj_waiters(sched, h, n);
    }
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
    let binding = api.binding(idx);
    let entry = &binding.entry;
    crate::win32::x86::host::poll(mem, state, sched);
    let dll = entry.dll.as_str();
    let name = entry.name.as_str();
    if dll == "user32.dll"
        && (matches!(
            binding.typed,
            Some(crate::win32::catalog::ApiId::Message(id)) if id.service_sent()
        ) || matches!(
            binding.typed,
            Some(crate::win32::catalog::ApiId::WindowControl(
                crate::win32::catalog::WindowControlApi::SendA
                    | crate::win32::catalog::WindowControlApi::SendW
            ))
        ))
        && crate::user32::service_sent(cpu, mem, api, state, log, sched, max_instr)?.is_some()
    {
        return Ok(());
    }
    let esp = cpu.esp();
    if cpu.fs_base != 0 {
        state.last_error = mem
            .read_u32(cpu.fs_base + 0x34)
            .map_err(|e| e.to_string())?;
    }
    let mut args = [0u32; 16];
    let stack_bytes = binding.stack_bytes;
    let cdecl = dll.contains("crt") || dll.starts_with("msvcr");
    let count = if dll == "user32.dll" && matches!(name, "wsprintfA" | "wsprintfW") {
        2 // cdecl varargs are read on demand by the formatter
    } else if cdecl {
        match name {
            "_errno" | "_CIpow" | "_ftol" | "_ftol2" | "_ftol2_sse" => 0,
            "floor" => 2,
            "ldexp" => 3,
            "pow" | "qsort" => 4,
            "memcmp" | "memcpy" | "memmove" | "memset" => 3,
            "toupper" | "tolower" => 1,
            _ => 12,
        }
    } else {
        stack_bytes.map_or(12, |n| (n / 4).min(16) as usize)
    };
    for (i, a) in args.iter_mut().take(count).enumerate() {
        let input = mem.read_u32(esp.wrapping_add(4 + 4 * i as u32));
        *a = if stack_bytes.is_some() && !cdecl {
            input.map_err(|e| e.to_string())?
        } else {
            input.unwrap_or(0xdead_beef)
        };
    }
    let mut implemented = true;
    let mut note = String::new();
    let mut yield_after_return = false;
    let ret: u32;
    // Match the bound API ID directly so typed calls do not walk a guard chain.
    // Legacy patterns keep their original order and precedence.
    match (binding.typed, dll, name) {
        _ if binding.unimplemented => {
            // The catalog already resolved this ABI-only export during binding.
            // Preserve its legacy error, return, trace and frame submission.
            implemented = false;
            state.last_error = 120;
            ret = if dll == "ole32.dll" { 0x80004001 } else { 0 };
        }
        (Some(crate::win32::catalog::ApiId::VirtualMemory(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::VirtualMemory(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) =
                crate::win32::x86::virtual_memory::call(id, &args, mem, &mut state.last_error);
        }
        (Some(crate::win32::catalog::ApiId::Heap(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Heap(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::win32::x86::heaps::call(
                id,
                &args,
                mem,
                &mut state.heap,
                state.process_heap,
                &mut state.last_error,
            )?;
        }
        (Some(crate::win32::catalog::ApiId::LocalMemory(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::LocalMemory(id)) = binding.typed else {
                unreachable!();
            };
            ret = crate::win32::x86::local_memory::call(
                id,
                &args,
                mem,
                &mut state.heap,
                state.process_heap,
            )?;
        }
        (Some(crate::win32::catalog::ApiId::File(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::File(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::file_api::call(id, &args, mem, state);
        }
        (Some(crate::win32::catalog::ApiId::Path(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Path(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::file_api::call_path(id, &args, mem, state);
        }
        (Some(crate::win32::catalog::ApiId::Interlocked(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Interlocked(id)) = binding.typed else {
                unreachable!();
            };
            ret = crate::win32::x86::interlocked::call(id, &args, mem)?;
        }
        (Some(crate::win32::catalog::ApiId::Context(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Context(id)) = binding.typed else {
                unreachable!();
            };
            use crate::win32::catalog::ContextApi;
            ret = match id {
                ContextApi::ProcessId => {
                    note = "fake pid".into();
                    4242
                }
                ContextApi::ThreadId => sched.current_tid(),
                ContextApi::ProcessHandle => u32::MAX,
                ContextApi::ThreadHandle => u32::MAX - 1,
                ContextApi::DebuggerPresent => 0,
                ContextApi::GetLastError => state.last_error,
                ContextApi::SetLastError => {
                    state.last_error = args[0];
                    0
                }
            };
        }
        (Some(crate::win32::catalog::ApiId::Local(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Local(id)) = binding.typed else {
                unreachable!();
            };
            use crate::win32::catalog::LocalApi::*;
            use crate::win32::kernel::locals32::LocalKind::{Fls, Tls};
            ret = match id {
                TlsAlloc => state.locals.allocate(Tls),
                TlsSet => {
                    state.locals.set(Tls, sched.current_tid(), args[0], args[1]);
                    1
                }
                TlsGet => {
                    let value = state.locals.get(Tls, sched.current_tid(), args[0]);
                    state.last_error = 0;
                    value
                }
                TlsFree => {
                    state.locals.free(Tls, args[0]);
                    1
                }
                FlsAlloc => {
                    let value = state.locals.allocate(Fls);
                    note = "callback not invoked".into();
                    value
                }
                FlsSet => {
                    state.locals.set(Fls, sched.current_tid(), args[0], args[1]);
                    1
                }
                FlsGet => state.locals.get(Fls, sched.current_tid(), args[0]),
                FlsFree => {
                    state.locals.free(Fls, args[0]);
                    1
                }
            };
        }
        (Some(crate::win32::catalog::ApiId::Clock(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Clock(id)) = binding.typed else {
                unreachable!();
            };
            use crate::win32::x86::clocks::ClockReturn;
            match crate::win32::x86::clocks::call(id, &args, mem, &state.start)? {
                ClockReturn::Dword(value) => ret = value,
                ClockReturn::Qword(value) => {
                    cpu.regs[EDX] = (value >> 32) as u32;
                    ret = value as u32;
                }
            }
        }
        (Some(crate::win32::catalog::ApiId::Guid(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Guid(id)) = binding.typed else {
                unreachable!();
            };
            match crate::win32::x86::guid::call(id, &args, mem, &mut state.heap, state.process_heap)
            {
                Ok(value) => {
                    ret = value;
                    note = "common process/string/COM allocation contract".into();
                }
                Err(error) => {
                    state.last_error = error;
                    ret = error;
                    note = format!("common API error {error:#x}");
                }
            }
        }
        (Some(crate::win32::catalog::ApiId::Version(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Version(id)) = binding.typed else {
                unreachable!();
            };
            let value = crate::win32::x86::version::call(
                id,
                &args,
                mem,
                &state.environment,
                &mut state.last_error,
            )?;
            ret = value.value;
            note = value.note.into();
        }
        (Some(crate::win32::catalog::ApiId::Nls(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Nls(id)) = binding.typed else {
                unreachable!();
            };
            let value = crate::win32::x86::nls::call(
                id,
                &args,
                mem,
                &state.environment,
                &mut state.last_error,
            )?;
            ret = value.value;
            note = value.note;
        }
        (Some(crate::win32::catalog::ApiId::SystemProfile(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::SystemProfile(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) =
                crate::win32::x86::system_profile::call(id, args[0], mem, &mut state.last_error);
        }
        (Some(crate::win32::catalog::ApiId::Calendar(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Calendar(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) =
                crate::win32::x86::calendar::call(id, args[0], mem, &mut state.last_error);
        }
        (Some(crate::win32::catalog::ApiId::Multimedia(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Multimedia(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::win32::x86::multimedia::call(id, &args, mem);
        }
        (Some(crate::win32::catalog::ApiId::CriticalSection(_)), _, _) => {
            use crate::win32::x86::critical_sections::{call, Outcome};
            let Some(crate::win32::catalog::ApiId::CriticalSection(id)) = binding.typed else {
                unreachable!();
            };
            match call(id, &args, mem, sched.current_tid())? {
                Outcome::Value(value) => ret = value,
                Outcome::Block { address: cs, owner } => {
                    sched.take_wake();
                    log.note(&format!(
                        "thread {} blocks on CRITICAL_SECTION {cs:#x} (owner tid {owner})",
                        sched.current_tid()
                    ));
                    sched.block_current(cpu, WaitTarget::CriticalSection(cs), None)?;
                    return Ok(());
                }
                Outcome::WakeOne(cs) => {
                    for i in 0..sched.threads.len() {
                        if matches!(&sched.threads[i].state, ThreadState::Waiting {
                            target: WaitTarget::CriticalSection(a), ..
                        } if *a == cs)
                        {
                            sched.wake_thread(i, Wake::Retest);
                            break;
                        }
                    }
                    ret = 0;
                }
            }
        }
        (Some(crate::win32::catalog::ApiId::ThreadProperty(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::ThreadProperty(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::win32::x86::thread_properties::call(
                id,
                &args,
                mem,
                sched,
                &mut state.last_error,
            )?;
        }
        (
            _,
            "winmm.dll",
            "mciSendCommandA" | "mciSendCommandW" | "mciSendStringA" | "mciSendStringW",
        ) => {
            ret = 274;
            implemented = false;
            note = "MCI device backend unavailable".into();
        }
        (_, "kernel32.dll", "TerminateProcess") => {
            if args[0] == u32::MAX
                || matches!(state.handles.get(&args[0]), Some(HandleObj::Process { .. }))
            {
                cpu.halted = true;
                cpu.exit_code = Some(args[1]);
                ret = 1;
                note = "terminated current guest process".into();
            } else {
                state.last_error = 6;
                ret = 0;
            }
        }
        (Some(crate::win32::catalog::ApiId::Com(id)), _, _)
            if id != crate::win32::catalog::ComApi::CreateInstance =>
        {
            use crate::win32::catalog::{ApiId, ComApi};
            let Some(ApiId::Com(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = match id {
                ComApi::Initialize | ComApi::InitializeEx | ComApi::Uninitialize => {
                    crate::win32::x86::com::apartment_call(
                        id,
                        &args,
                        &mut state.apartments,
                        sched.current_tid(),
                        &mut state.last_error,
                    )
                }
                ComApi::TaskAlloc | ComApi::TaskFree => crate::win32::x86::com::task_call(
                    id,
                    &args,
                    mem,
                    &mut state.heap,
                    state.process_heap,
                    &mut state.last_error,
                ),
                _ => unreachable!("PE32 COM export has no typed route"),
            };
        }
        (Some(crate::win32::catalog::ApiId::Bstr(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Bstr(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::win32::x86::bstr::call(
                id,
                &args,
                mem,
                &mut state.heap,
                &mut state.bstrs,
                state.process_heap,
                &mut state.last_error,
            );
        }
        (Some(crate::win32::catalog::ApiId::Variant(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Variant(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::win32::x86::variant::call(
                id,
                &args,
                mem,
                &mut state.heap,
                &mut state.bstrs,
                state.process_heap,
                &mut state.last_error,
            );
        }
        (
            Some(
                crate::win32::catalog::ApiId::BasicString(_)
                | crate::win32::catalog::ApiId::PointerHelper(_)
                | crate::win32::catalog::ApiId::ProcessQuery(_),
            ),
            _,
            _,
        ) => {
            use crate::win32::catalog::{ApiId, ProcessQueryApi};
            let id = binding.typed.unwrap();
            let result = match id {
                ApiId::BasicString(id) => crate::win32::x86::basic_strings::call(id, &args, mem),
                ApiId::PointerHelper(id) => {
                    crate::win32::x86::pointer_helpers::call(id, &args, mem)
                }
                ApiId::ProcessQuery(id) => {
                    use crate::win32::x86::process_queries::ProcessQueryResult;
                    crate::win32::x86::process_queries::call(id, &args, mem, &state.handles, sched)
                        .map(|outcome| match outcome {
                            ProcessQueryResult::Value(value) => value,
                            ProcessQueryResult::Open { access } => {
                                state.alloc_handle(HandleObj::Process { access })
                            }
                        })
                }
                _ => unreachable!(),
            };
            match result {
                Ok(value) => {
                    ret = value;
                    note = "common process/string/COM allocation contract".into();
                }
                Err(error) => {
                    state.last_error = error;
                    ret = if id == ApiId::ProcessQuery(ProcessQueryApi::SetIdealProcessor) {
                        u32::MAX
                    } else {
                        0
                    };
                    note = format!("common API error {error:#x}");
                }
            }
        }
        (Some(crate::win32::catalog::ApiId::Geometry(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Geometry(id)) = binding.typed else {
                unreachable!()
            };
            let result = crate::win32::x86::geometry::call(id, &args, mem, &mut state.last_error)?;
            ret = result.value;
            note = result.note.into();
        }
        (Some(crate::win32::catalog::ApiId::Cursor(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Cursor(id)) = binding.typed else {
                unreachable!()
            };
            let result = crate::win32::x86::cursor::call(
                id,
                &args,
                mem,
                &mut state.user32.cursor,
                sched.current_tid(),
                &mut state.last_error,
            )?;
            ret = result.value;
            note = result.note.into();
        }
        (Some(crate::win32::catalog::ApiId::Display(_)), _, _) => {
            use crate::win32::catalog::{ApiId, DisplayApi};
            let Some(ApiId::Display(id)) = binding.typed else {
                unreachable!();
            };
            if id == DisplayApi::Monitors {
                let (outcome, message) = crate::user32::enumerate_monitors(
                    &args, cpu, mem, api, state, log, sched, max_instr,
                )?;
                match outcome {
                    GuestCallOutcome::Returned(value) => {
                        ret = value;
                        note = message;
                    }
                    _ => return Ok(()),
                }
            } else {
                let result =
                    crate::win32::x86::display::call(id, &args, mem, &mut state.last_error)?;
                ret = result.value;
                note = result.note;
            }
        }
        (Some(crate::win32::catalog::ApiId::GdiDc(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::GdiDc(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::win32::gdi::dc_x86::call(
                id,
                &args,
                &mut state.graphics,
                |hwnd| {
                    state
                        .user32
                        .windows
                        .get(&hwnd)
                        .map(|window| window.client_size())
                },
                &mut state.last_error,
            );
        }
        (Some(crate::win32::catalog::ApiId::GdiOperation(id)), _, _) => {
            (ret, note, implemented) = crate::win32::gdi::x86::call(
                id,
                &args,
                mem,
                &mut state.graphics,
                &mut state.last_error,
            );
        }
        (Some(crate::win32::catalog::ApiId::UserObject(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::UserObject(id)) = binding.typed else {
                unreachable!();
            };
            let result = state
                .user32
                .object_call(id, &args, mem, &mut state.last_error)?;
            ret = result.0;
            note = result.1.into();
        }
        (Some(crate::win32::catalog::ApiId::WindowQuery(_)), _, _) => {
            use crate::win32::catalog::{ApiId, WindowQueryApi};
            let Some(ApiId::WindowQuery(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = if id == WindowQueryApi::Desktop {
                crate::user32::desktop_query(mem, api, state)?
            } else {
                state
                    .user32
                    .query_call(id, &args, mem, &mut state.last_error)?
            };
            implemented = id != WindowQueryApi::Placement;
        }
        (Some(crate::win32::catalog::ApiId::WindowControl(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::WindowControl(id)) = binding.typed else {
                unreachable!();
            };
            let (outcome, message) = crate::user32::control_call(
                id, &args, cpu, mem, api, state, log, sched, max_instr,
            )?;
            match outcome {
                GuestCallOutcome::Returned(value) => {
                    ret = value;
                    note = message;
                }
                _ => return Ok(()),
            }
        }
        (Some(crate::win32::catalog::ApiId::WindowState(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::WindowState(id)) = binding.typed else {
                unreachable!();
            };
            let (outcome, message) = crate::user32::window_state_call(
                id, &args, cpu, mem, api, state, log, sched, max_instr,
            )?;
            match outcome {
                GuestCallOutcome::Returned(value) => {
                    ret = value;
                    note = message;
                }
                _ => return Ok(()),
            }
        }
        (Some(crate::win32::catalog::ApiId::Dialog(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Dialog(id)) = binding.typed else {
                unreachable!();
            };
            implemented = !id.unavailable();
            let (outcome, message) =
                crate::user32::dialog_call(id, &args, cpu, mem, api, state, log, sched, max_instr)?;
            match outcome {
                GuestCallOutcome::Returned(value) => {
                    ret = value;
                    note = message;
                }
                _ => return Ok(()),
            }
        }
        (Some(crate::win32::catalog::ApiId::Painting(id)), _, _) => {
            let (outcome, message) =
                crate::user32::paint_call(id, &args, cpu, mem, api, state, log, sched, max_instr)?;
            match outcome {
                GuestCallOutcome::Returned(value) => {
                    ret = value;
                    note = message;
                }
                _ => return Ok(()),
            }
        }
        (Some(crate::win32::catalog::ApiId::Menu(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Menu(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note, implemented) = state.menus.call(
                id,
                &args,
                mem,
                &mut state.user32.windows,
                &mut state.last_error,
            );
        }
        (Some(crate::win32::catalog::ApiId::Class(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Class(id)) = binding.typed else {
                unreachable!();
            };
            (ret, note) = crate::user32::class_call(id, &args, mem, api, state)?;
        }
        (Some(crate::win32::catalog::ApiId::Desktop(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Desktop(id)) = binding.typed else {
                unreachable!();
            };
            let result = crate::win32::x86::desktop::call(id);
            ret = result.0;
            note = result.1.into();
        }
        (Some(crate::win32::catalog::ApiId::Image(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Image(id)) = binding.typed else {
                unreachable!()
            };
            let result = crate::win32::x86::images::call(id, &args, &mut state.last_error);
            ret = result.value;
            note = result.note.into();
            implemented = result.implemented;
        }
        (_, "wire.wmi", _) => {
            (ret, note, implemented) =
                crate::wmi::dispatch(dll, name, &args, mem, api, state).unwrap();
        }
        (Some(crate::win32::catalog::ApiId::Keyboard(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Keyboard(id)) = binding.typed else {
                unreachable!();
            };
            ret = state
                .user32
                .keyboard_call(id, &args, mem, sched.current_tid())?;
            note = "host asynchronous / retrieved queue key state".into();
        }
        (
            Some(crate::win32::catalog::ApiId::Com(crate::win32::catalog::ComApi::CreateInstance)),
            _,
            _,
        ) => {
            (ret, note, implemented) =
                crate::wmi::dispatch(dll, name, &args, mem, api, state).unwrap();
        }
        (_, "user32.dll", "wsprintfA" | "wsprintfW" | "wvsprintfA" | "wvsprintfW") => {
            let varargs = if name.starts_with("wvs") {
                args[2]
            } else {
                esp.checked_add(12).ok_or("wsprintf stack overflow")?
            };
            ret = crate::wsprintf::format(mem, args[0], args[1], varargs, name.ends_with('W'))?;
            note = "checked guest Windows varargs formatting".into();
        }
        (Some(crate::win32::catalog::ApiId::Registry(id)), _, _) => {
            (ret, note) = crate::win32::x86::registry::call(id, &args, mem)?;
        }
        (
            _,
            "version.dll",
            "GetFileVersionInfoA"
            | "GetFileVersionInfoW"
            | "GetFileVersionInfoSizeA"
            | "GetFileVersionInfoSizeW"
            | "VerQueryValueA"
            | "VerQueryValueW",
        ) => {
            (ret, note) = crate::version::dispatch(name, &args, mem, state).unwrap();
        }
        (_, "wire", "ExitEmulation") => {
            log.log_call(
                idx,
                dll,
                name,
                &args[..1],
                0,
                true,
                "initial thread returned",
            );
            match crate::modules::notify_process_exit(cpu, mem, api, state, log, sched, max_instr)?
            {
                GuestCallOutcome::Returned(_) => {}
                _ => return Ok(()),
            }
            cpu.halted = true;
            cpu.exit_code = Some(0);
            return Ok(());
        }
        (_, "wire", "ExitThread") => {
            // A worker thread function returned; its EAX is the exit code.
            let code = cpu.regs[0];
            log.log_call(
                idx,
                dll,
                name,
                &args[..1],
                0,
                true,
                "thread function returned",
            );
            match crate::modules::notify_thread(
                false, false, cpu, mem, api, state, log, sched, max_instr,
            )? {
                GuestCallOutcome::Returned(_) => {}
                _ => return Ok(()),
            }
            state
                .modules
                .release_tls_thread(&mut state.heap, state.process_heap, cpu.fs_base);
            crate::user32::thread_exit(state, sched, sched.current_tid());
            sched.exit_current(cpu, code)?;
            return Ok(());
        }
        (_, "kernel32.dll", "ExitProcess") => {
            log.log_call(idx, dll, name, &args[..1], 0, true, "process exit");
            match crate::modules::notify_process_exit(cpu, mem, api, state, log, sched, max_instr)?
            {
                GuestCallOutcome::Returned(_) => {}
                _ => return Ok(()),
            }
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            return Ok(());
        }
        (
            Some(crate::win32::catalog::ApiId::ThreadControl(
                crate::win32::catalog::ThreadControlApi::Exit,
            )),
            _,
            _,
        ) => {
            log.log_call(idx, dll, name, &args[..1], 0, true, "thread exit");
            match crate::modules::notify_thread(
                false, false, cpu, mem, api, state, log, sched, max_instr,
            )? {
                GuestCallOutcome::Returned(_) => {}
                _ => return Ok(()),
            }
            state
                .modules
                .release_tls_thread(&mut state.heap, state.process_heap, cpu.fs_base);
            crate::user32::thread_exit(state, sched, sched.current_tid());
            sched.exit_current(cpu, args[0])?;
            return Ok(());
        }
        (
            Some(crate::win32::catalog::ApiId::ThreadControl(
                crate::win32::catalog::ThreadControlApi::Create,
            )),
            _,
            _,
        ) => {
            if !sched.has_thread_capacity() {
                state.last_error = 8;
                ret = 0;
                note = "guest thread resource limit".into();
            } else {
                let (entry, param, flags) = (args[2], args[3], args[4]);
                let tid = sched.alloc_tid();
                let exit_stub = api.lookup("wire", "ExitThread").unwrap();
                let handle = state.alloc_handle(HandleObj::Thread { tid });
                sched.create_thread(mem, entry, param, flags & 4 != 0, tid, handle, exit_stub)?;
                let teb = sched.threads.last().unwrap().teb;
                state
                    .modules
                    .setup_tls_thread(mem, &mut state.heap, state.process_heap, teb)?;
                if args[5] != 0 {
                    mem.write_u32(args[5], tid).map_err(|e| e.to_string())?;
                }
                note = format!(
                    "tid {tid} entry={entry:#x} param={param:#x}{}",
                    if flags & 4 != 0 { " (suspended)" } else { "" }
                );
                ret = handle;
            }
        }
        (
            Some(crate::win32::catalog::ApiId::Delay(
                crate::win32::catalog::DelayApi::SwitchThread,
            )),
            _,
            _,
        ) => {
            sched.poll_timeouts();
            ret = sched.any_other_runnable() as u32;
            yield_after_return = ret != 0;
        }
        (
            Some(crate::win32::catalog::ApiId::Delay(crate::win32::catalog::DelayApi::Sleep)),
            _,
            _,
        ) => {
            if sched.take_wake().is_some() {
                ret = 0; // void; woke after the full delay
            } else if args[0] == 0 {
                yield_after_return = true;
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
        (
            Some(crate::win32::catalog::ApiId::Delay(crate::win32::catalog::DelayApi::SleepEx)),
            _,
            _,
        ) => {
            if sched.take_wake().is_some() {
                ret = 0;
            } else if args[0] == 0 {
                yield_after_return = true;
                ret = 0;
            } else {
                let d = sched.arm_deadline(args[0]);
                sched.block_current(cpu, WaitTarget::Sleep, d)?;
                return Ok(());
            }
        }
        // --- events and semaphores: resolved once by the catalog ---
        (Some(crate::win32::catalog::ApiId::SyncObject(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::SyncObject(id)) = binding.typed else {
                unreachable!();
            };
            use crate::win32::catalog::SyncObjectApi::*;
            ret = match id {
                CreateEventA | CreateEventW => {
                    let nm = read_str(mem, args[3], id.wide());
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
                    h
                }
                OpenEventA | OpenEventW => {
                    let nm = read_str(mem, args[2], id.wide());
                    match state.named_objs.get(&nm) {
                        Some(&h) => {
                            note = format!("event \"{nm}\"");
                            h
                        }
                        None => {
                            state.last_error = 2; // ERROR_FILE_NOT_FOUND
                            note = format!("event \"{nm}\" not found");
                            0
                        }
                    }
                }
                SetEvent => match state.handles.get_mut(&args[0]) {
                    Some(HandleObj::Event { manual, signaled }) => {
                        let manual = *manual;
                        *signaled = true;
                        let woken =
                            wake_obj_waiters(sched, args[0], if manual { usize::MAX } else { 1 });
                        note = format!("woke {woken} waiter(s)");
                        1
                    }
                    _ => {
                        state.last_error = 6;
                        note = "bad event handle".into();
                        0
                    }
                },
                ResetEvent => match state.handles.get_mut(&args[0]) {
                    Some(HandleObj::Event { signaled, .. }) => {
                        *signaled = false;
                        1
                    }
                    _ => {
                        state.last_error = 6;
                        0
                    }
                },
                PulseEvent => match state.handles.get_mut(&args[0]) {
                    Some(HandleObj::Event { manual, signaled }) => {
                        let manual = *manual;
                        *signaled = false;
                        let woken =
                            wake_obj_waiters(sched, args[0], if manual { usize::MAX } else { 1 });
                        note = format!("woke {woken} waiter(s)");
                        1
                    }
                    _ => {
                        state.last_error = 6;
                        0
                    }
                },
                CreateSemaphoreW | CreateSemaphoreA => {
                    let nm = read_str(mem, args[3], id.wide());
                    let (initial, max) = (args[1] as i32, args[2] as i32);
                    let (h, existed) = state.create_named(&nm, || HandleObj::Semaphore {
                        count: initial,
                        max,
                    });
                    note = format!(
                        "semaphore \"{nm}\" {initial}/{max}{}",
                        if existed { " (already exists)" } else { "" }
                    );
                    h
                }
                ReleaseSemaphore => match state.handles.get_mut(&args[0]) {
                    Some(HandleObj::Semaphore { count, max }) => {
                        let prev = *count;
                        let n = args[1] as i32;
                        *count = (*count + n).min(*max);
                        if args[2] != 0 {
                            mem.write_u32(args[2], prev as u32)
                                .map_err(|e| e.to_string())?;
                        }
                        let woken = wake_obj_waiters(sched, args[0], n.max(0) as usize);
                        note = format!("count {prev} -> {}, woke {woken}", *count);
                        1
                    }
                    _ => {
                        state.last_error = 6;
                        note = "bad semaphore handle".into();
                        0
                    }
                },
                CreateEventExA | CreateEventExW | CreateSemaphoreExA | CreateSemaphoreExW
                | OpenSemaphoreA | OpenSemaphoreW => {
                    unreachable!("ABI-only exports use the legacy unimplemented policy")
                }
            };
        }
        (Some(crate::win32::catalog::ApiId::Mutex(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Mutex(id)) = binding.typed else {
                unreachable!();
            };
            use crate::win32::catalog::MutexApi::*;
            ret = match id {
                CreateA | CreateW => {
                    let nm = read_str(mem, args[2], id == CreateW);
                    let me = sched.current_tid();
                    let own = args[1] != 0;
                    let (h, existed) = state.create_named(&nm, || HandleObj::Mutex {
                        owner: if own { Some(me) } else { None },
                        count: if own { 1 } else { 0 },
                    });
                    note = format!(
                        "mutex \"{nm}\"{}",
                        if existed { " (already exists)" } else { "" }
                    );
                    h
                }
                Release => {
                    let me = sched.current_tid();
                    match state.handles.get_mut(&args[0]) {
                        Some(HandleObj::Mutex { owner, count }) if *owner == Some(me) => {
                            *count -= 1;
                            if *count == 0 {
                                *owner = None;
                                wake_obj_waiters(sched, args[0], 1);
                            }
                            1
                        }
                        Some(HandleObj::Mutex { .. }) => {
                            state.last_error = 288; // ERROR_NOT_OWNER
                            note = "not the owner".into();
                            0
                        }
                        _ => {
                            state.last_error = 6;
                            note = "bad mutex handle".into();
                            0
                        }
                    }
                }
                CreateExA | CreateExW | OpenA | OpenW => {
                    unreachable!("ABI-only exports use the legacy unimplemented policy")
                }
            };
        }
        // --- waits ---
        (
            Some(crate::win32::catalog::ApiId::Wait(
                crate::win32::catalog::WaitApi::Single | crate::win32::catalog::WaitApi::SingleEx,
            )),
            _,
            _,
        ) => {
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
                            if timeout == u32::MAX {
                                "inf".into()
                            } else {
                                timeout.to_string()
                            }
                        ));
                        sched.block_current(cpu, WaitTarget::Object(h), d)?;
                        return Ok(());
                    }
                }
            }
        }
        (
            Some(crate::win32::catalog::ApiId::Wait(crate::win32::catalog::WaitApi::Multiple)),
            _,
            _,
        ) => {
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
        (Some(crate::win32::catalog::ApiId::Message(id)), _, _)
            if !matches!(
                id,
                crate::win32::catalog::MessageApi::WaitObjects
                    | crate::win32::catalog::MessageApi::WaitObjectsEx
            ) =>
        {
            let Some(crate::win32::catalog::ApiId::Message(id)) = binding.typed else {
                unreachable!();
            };
            let (outcome, message) = crate::user32::message_call(
                id, &args, cpu, mem, api, state, log, sched, max_instr,
            )?;
            match outcome {
                GuestCallOutcome::Returned(value) => {
                    ret = value;
                    note = message;
                }
                _ => return Ok(()),
            }
        }
        (
            Some(crate::win32::catalog::ApiId::Message(
                crate::win32::catalog::MessageApi::WaitObjects
                | crate::win32::catalog::MessageApi::WaitObjectsEx,
            )),
            _,
            _,
        ) => {
            let (n, ptr, timeout, mask, flags) = if binding.typed
                == Some(crate::win32::catalog::ApiId::Message(
                    crate::win32::catalog::MessageApi::WaitObjectsEx,
                )) {
                (args[0], args[1], args[2], args[3], args[4])
            } else {
                (
                    args[0],
                    args[1],
                    args[3],
                    args[4],
                    if args[2] != 0 { 1 } else { 0 },
                )
            };
            sched.take_wake();
            if n > 63 || flags & !7 != 0 || flags & 2 != 0 || mask & !0x5ff != 0 {
                sched.clear_deadline();
                state.last_error = 87;
                ret = u32::MAX;
            } else {
                let hs = (0..n)
                    .map(|i| mem.read_u32(ptr + i * 4).map_err(|e| e.to_string()))
                    .collect::<Result<Vec<_>, _>>()?;
                let tid = sched.current_tid();
                let input = crate::user32::message_wait_ready(state, tid, mask, flags & 4 != 0);
                let invalid = hs.iter().any(|h| !state.handles.contains_key(h));
                let got = if invalid {
                    None
                } else if flags & 1 != 0 {
                    if input && hs.iter().all(|&h| obj_ready(state, sched, h)) {
                        for &h in &hs {
                            obj_acquire(state, sched, h);
                        }
                        Some(0)
                    } else {
                        None
                    }
                } else {
                    hs.iter()
                        .enumerate()
                        .find_map(|(i, &h)| obj_acquire(state, sched, h).then_some(i as u32))
                        .or(input.then_some(n))
                };
                if invalid {
                    sched.clear_deadline();
                    state.last_error = 6;
                    ret = u32::MAX;
                } else if let Some(r) = got {
                    sched.clear_deadline();
                    ret = r;
                } else {
                    let deadline = sched.arm_deadline(timeout);
                    if timeout == 0 || deadline.is_some_and(|d| d <= Instant::now()) {
                        sched.clear_deadline();
                        ret = WAIT_TIMEOUT;
                    } else {
                        let timer = state
                            .user32
                            .timers
                            .iter()
                            .filter(|((owner, _, _), _)| *owner == tid && mask & 0x10 != 0)
                            .map(|(_, t)| t.deadline)
                            .min();
                        let wake = match (deadline, timer) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (a, b) => a.or(b),
                        };
                        sched.block_current(
                            cpu,
                            WaitTarget::MessageObjects(hs, tid, mask),
                            wake,
                        )?;
                        return Ok(());
                    }
                }
                note = "checked object/message wait".into();
            }
        }
        (
            Some(crate::win32::catalog::ApiId::Handle(
                crate::win32::catalog::HandleApi::CloseHandle,
            )),
            _,
            _,
        ) => {
            if state.handles.remove(&args[0]).is_some() {
                state.named_objs.retain(|_, handle| *handle != args[0]);
                ret = 1;
            } else {
                state.last_error = 6;
                ret = 0;
                note = "unknown handle".into();
            }
        }
        (Some(crate::win32::catalog::ApiId::Startup(id)), _, _) => {
            let result = crate::win32::x86::process_startup::call(
                id,
                &args,
                mem,
                [state.cmdline_a, state.cmdline_w],
            )?;
            ret = result.0;
            note = result.1.into();
        }
        (Some(crate::win32::catalog::ApiId::Resource(id)), _, _) => {
            (ret, note) = crate::win32::x86::resources::call(
                id,
                &args,
                mem,
                state.exe_base,
                &mut state.last_error,
            )?;
        }
        (Some(crate::win32::catalog::ApiId::Module(_)), _, _) => {
            let Some(crate::win32::catalog::ApiId::Module(id)) = binding.typed else {
                unreachable!();
            };
            let (outcome, message) =
                crate::modules::call(id, &args, cpu, mem, api, state, log, sched, max_instr)?;
            match outcome {
                crate::cpu::GuestCallOutcome::Returned(value) => {
                    ret = value;
                    note = message;
                }
                _ => return Ok(()),
            }
        }
        (
            Some(crate::win32::catalog::ApiId::Handle(
                crate::win32::catalog::HandleApi::GetStdHandle,
            )),
            _,
            _,
        ) => {
            ret = 0;
            note = "fake null handle".into();
        }
        (_, "kernel32.dll", "SetHandleCount") => {
            // Legacy CRT hint; NT-style handle tables grow automatically.
            ret = args[0];
            note = "automatic handle table capacity".into();
        }
        (_, "kernel32.dll", "EncodePointer") | (_, "kernel32.dll", "DecodePointer") => {
            ret = args[0];
            note = "identity encoding".into();
        }
        (Some(crate::win32::catalog::ApiId::Environment(id)), _, _) => {
            let storage = if id.scratch() {
                state.scratch_alloc(mem, 16, 16)
            } else {
                0
            };
            let result = crate::win32::x86::environment::call(id, mem, storage)?;
            ret = result.0;
            note = result.1.into();
        }
        (_, "kernel32.dll", "SetUnhandledExceptionFilter") => {
            ret = state.unhandled_filter;
            state.unhandled_filter = args[0];
        }
        (_, "kernel32.dll", "RaiseException") => {
            log.log_call(
                idx,
                dll,
                name,
                &args[..4],
                0,
                true,
                "first-chance SEH dispatch",
            );
            return crate::seh::raise(cpu, mem, api, state, log, sched, &args, max_instr);
        }
        (_, "kernel32.dll", "UnhandledExceptionFilter") => {
            if state.unhandled_filter == 0 {
                ret = 0;
            } else {
                let saved = cpu.save_ctx();
                let filter = state.unhandled_filter;
                match cpu.call_guest(mem, api, state, log, sched, filter, &[args[0]], max_instr) {
                    Ok(crate::cpu::GuestCallOutcome::Returned(value)) => {
                        cpu.load_ctx(&saved);
                        ret = value;
                    }
                    Ok(_) => return Ok(()),
                    Err(error) => {
                        cpu.load_ctx(&saved);
                        return Err(error);
                    }
                }
            }
        }
        (_, "kernel32.dll", "RtlUnwind") | (_, "ntdll.dll", "RtlUnwind") => {
            log.log_call(idx, dll, name, &args[..4], args[3], true, "SEH unwind");
            return crate::seh::unwind(cpu, mem, api, state, log, sched, &args, max_instr);
        }
        (_, "kernel32.dll", "GetCurrentPackageId") => {
            ret = 0x3d54; // APPMODEL_ERROR_NO_PACKAGE (desktop app)
            note = "not a packaged app".into();
        }
        (_, "user32.dll", "GetProcessWindowStation") => {
            ret = 0x7007_0000;
            note = "fake winsta handle".into();
        }
        (_, "user32.dll", "GetUserObjectInformationW") => {
            ret = 0;
            note = "fake: unsupported winsta query".into();
        }
        (_, "user32.dll", "MessageBoxA") | (_, "user32.dll", "MessageBoxW") => {
            let wide = name.ends_with('W');
            let text = read_str(mem, args[1], wide);
            let caption = read_str(mem, args[2], wide);
            note = format!("SHOWN: [{caption}] {text}");
            ret = 1; // IDOK
        }
        (_, "mpr.dll", "WNetGetUniversalNameA") | (_, "mpr.dll", "WNetGetUniversalNameW") => {
            ret = 2250; // ERROR_NOT_CONNECTED: sandbox C: is a local drive
        }
        (_, "kernel32.dll", "FormatMessageA") | (_, "kernel32.dll", "FormatMessageW") => {
            let wide = name.ends_with('W');
            let text = if args[0] & 0x400 != 0 {
                crate::file_api::read_name(mem, args[1], wide)
                    .map_err(|e| format!("FormatMessage: {e}"))?
            } else {
                match args[2] {
                    2 => "The system cannot find the file specified.\r\n".into(),
                    5 => "Access is denied.\r\n".into(),
                    87 => "The parameter is incorrect.\r\n".into(),
                    n => format!("Win32 error {n}.\r\n"),
                }
            };
            let count = if wide {
                text.encode_utf16().count()
            } else {
                crate::codepage::encode(&text, 932)
                    .map_err(|e| format!("FormatMessage: {e}"))?
                    .0
                    .len()
            } as u32;
            if args[0] & 0x100 != 0 {
                let p = state.heap.alloc(
                    mem,
                    state.process_heap,
                    (count + 1).max(args[5]) * if wide { 2 } else { 1 },
                    false,
                )?;
                if p == 0 {
                    state.last_error = 8;
                    ret = 0;
                } else {
                    crate::file_api::write_string(mem, p, count + 1, &text, wide)
                        .map_err(|e| format!("FormatMessage: {e}"))?;
                    mem.write_u32(args[4], p).map_err(|e| e.to_string())?;
                    ret = count;
                }
            } else if args[5] <= count {
                state.last_error = 122;
                ret = 0;
            } else {
                crate::file_api::write_string(mem, args[4], args[5], &text, wide)
                    .map_err(|e| format!("FormatMessage: {e}"))?;
                ret = count;
            }
            note = text.trim().into();
        }
        (_, "mscoree.dll", "CorExitProcess") => {
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            note = "CLR-style exit".into();
            ret = 0;
        }
        (_, "gdiplus.dll", _) => {
            (ret, note, implemented) = crate::gdiplus::dispatch(name, &args, mem, state);
        }
        (_, "d3d9.dll", _) => {
            (ret, note, implemented) =
                crate::d3d9::dispatch(dll, name, &args, mem, api, state).unwrap();
        }
        (_, "dsound.dll", _) => {
            (ret, note, implemented) = crate::dsound::dispatch(name, &args, mem, api, state, sched);
        }
        (_, "winmm.dll", "waveOutGetNumDevs") => {
            ret = 0;
            note = "no audio output backend".into();
        }
        (_, "winmm.dll", "waveOutOpen") => {
            if args[0] != 0 {
                mem.write_u32(args[0], 0).map_err(|e| e.to_string())?;
            }
            ret = 6; // MMSYSERR_NODRIVER
            implemented = false;
            note = "wave output backend unavailable".into();
        }
        (
            _,
            "gdi32.dll",
            "AddFontMemResourceEx"
            | "RemoveFontMemResourceEx"
            | "AddFontResourceExA"
            | "AddFontResourceExW"
            | "RemoveFontResourceExA"
            | "RemoveFontResourceExW",
        ) => {
            state.last_error = 120;
            implemented = false;
            ret = 0;
            note = "font resource backend not ported".into();
        }
        _ => {
            if let Some((v, msg)) = crate::file_api::dispatch(dll, name, &args, mem, state) {
                ret = v;
                note = msg;
            } else if let Some((v, msg)) = crate::system::dispatch(dll, name, &args, mem, state) {
                ret = v;
                note = msg;
            } else if let Some((v, msg)) = crate::nls::dispatch(dll, name, &args, mem, state) {
                ret = v;
                note = msg;
            } else if let Some(result) = crate::user32::dispatch(
                dll, name, &args, cpu, mem, api, state, log, sched, max_instr,
            ) {
                let (outcome, message) = result?;
                match outcome {
                    crate::cpu::GuestCallOutcome::Returned(value) => {
                        ret = value;
                        note = message;
                    }
                    _ => return Ok(()),
                }
            } else {
                match crt_dispatch(
                    dll, name, &args, cpu, mem, api, state, log, sched, max_instr,
                )? {
                    Some(crate::cpu::GuestCallOutcome::Returned(v)) => {
                        ret = v;
                        note = "crt shim".into();
                    }
                    Some(_) => return Ok(()),
                    None => {
                        implemented = false;
                        state.last_error = 120;
                        ret = if dll == "ole32.dll" { 0x80004001 } else { 0 };
                        if stack_bytes.is_none() {
                            note = "unknown stdcall arity, assuming cdecl/0-byte cleanup".into();
                        }
                    }
                }
            }
        }
    }
    let stack_args = stack_bytes.unwrap_or(0);
    let nargs = (stack_args / 4).min(6) as usize;
    log.log_call(
        idx,
        dll,
        name,
        &args[..nargs.max(1)],
        ret,
        implemented,
        &note,
    );

    cpu.regs[0] = ret; // eax
    let retaddr = mem
        .read_u32(esp)
        .map_err(|e| format!("stub stack fault: {e}"))?;
    cpu.set_esp(esp.wrapping_add(4 + stack_args));
    cpu.eip = retaddr;
    if cpu.fs_base != 0 {
        mem.write_u32(cpu.fs_base + 0x34, state.last_error)
            .map_err(|e| e.to_string())?;
    }
    // Finish the caller's stdcall frame before saving its context. Switching
    // earlier would apply its EAX/ESP/return PC and LastError to another thread.
    if yield_after_return {
        sched.round_robin(cpu);
    }
    Ok(())
}

/// Minimal CRT (UCRT/msvcr) shims. All cdecl. Ok(None) = not covered (caller
/// logs it as unimplemented).
#[allow(clippy::too_many_arguments)]
fn crt_dispatch(
    dll: &str,
    name: &str,
    args: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<Option<crate::cpu::GuestCallOutcome>, String> {
    if !(dll.starts_with("api-ms-win-crt-")
        || dll == "ucrtbase.dll"
        || dll == "msvcrt.dll"
        || (dll.starts_with("msvcr") && dll.ends_with(".dll")))
    {
        return Ok(None);
    }
    let r = match name {
        "time" | "_time32" => {
            let n = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs();
            let value = if n > i32::MAX as u64 {
                u32::MAX
            } else {
                n as u32
            };
            if args[0] != 0 {
                mem.write_u32(args[0], value).map_err(|e| e.to_string())?;
            }
            value
        }
        "_mbsinc" => {
            let first = mem.read_u8(args[0]).map_err(|e| e.to_string())?;
            let lead = matches!(first, 0x81..=0x9f | 0xe0..=0xfc);
            let pair = lead
                && mem
                    .read_u8(args[0].checked_add(1).ok_or("_mbsinc address overflow")?)
                    .map_err(|e| e.to_string())?
                    != 0;
            args[0]
                .checked_add(if pair { 2 } else { 1 })
                .ok_or("_mbsinc address overflow")?
        }
        "_mbsnbcpy" => {
            // Hangover Wine mbcs.c, LGPL-2.1-or-later, Alexandre Julliard
            // (1999), Jon Griffths (2000). Checked CP932 guest adaptation.
            if args[2] == 0 {
                return Ok(Some(crate::cpu::GuestCallOutcome::Returned(args[0])));
            }
            if args[2] > 1 << 26 {
                return Err("_mbsnbcpy size too large".into());
            }
            let mut bytes = vec![0; args[2] as usize];
            let mut lead = false;
            let mut copied = 0;
            for i in 0..args[2] {
                let b = mem
                    .read_u8(args[1].checked_add(i).ok_or("_mbsnbcpy address overflow")?)
                    .map_err(|e| e.to_string())?;
                if b == 0 {
                    break;
                }
                lead = !lead && matches!(b, 0x81..=0x9f | 0xe0..=0xfc);
                bytes[i as usize] = b;
                copied += 1;
            }
            if lead {
                bytes[copied - 1] = 0;
            }
            mem.write_bytes(args[0], &bytes)
                .map_err(|e| e.to_string())?;
            args[0]
        }
        "_errno" => state.errno_address(mem, sched.current_tid())?,
        "_ftol" | "_ftol2" | "_ftol2_sse" => {
            let result = cpu.fpu.crt_ftol();
            cpu.regs[crate::cpu::EDX] = (result >> 32) as u32;
            result as u32
        }
        "floor" => {
            cpu.fpu.crt_floor(args[0] as u64 | ((args[1] as u64) << 32));
            0
        }
        "ldexp" => {
            if cpu
                .fpu
                .crt_ldexp(args[0] as u64 | ((args[1] as u64) << 32), args[2] as i32)
            {
                let errno = state.errno_address(mem, sched.current_tid())?;
                mem.write_u32(errno, 34).map_err(|e| e.to_string())?;
            }
            0
        }
        "_CIpow" | "pow" => {
            let values = (name == "pow").then_some((
                args[0] as u64 | ((args[1] as u64) << 32),
                args[2] as u64 | ((args[3] as u64) << 32),
            ));
            if let Some(error) = cpu.fpu.crt_pow(values) {
                let errno = state.errno_address(mem, sched.current_tid())?;
                mem.write_u32(errno, error).map_err(|e| e.to_string())?;
            }
            0
        }
        "qsort" => {
            return crate::crt_sort::qsort(
                args[0], args[1], args[2], args[3], cpu, mem, api, state, log, sched, max_instr,
            )
            .map(Some)
        }
        "__acrt_iob_func" => state
            .scratch_alloc(mem, 0x100, 16)
            .wrapping_add(args[0].wrapping_mul(0x40)),
        "__set_app_type" => {
            state.crt_app_type = args[0];
            0
        }
        "__p__fmode" | "__p__commode" | "__p___argc" | "__p__timezone" | "__p__daylight"
        | "__p__acmdln" | "__p__wcmdln" => api
            .data_export(mem, dll, &name[4..], state.cmdline_a, state.cmdline_w)?
            .ok_or("unknown CRT data accessor")?,
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
            mem.write_u32(p, state.cmdline_a)
                .map_err(|e| e.to_string())?;
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
                    let v = match cpu.call_guest(mem, api, state, log, sched, f, &[], max_instr)? {
                        crate::cpu::GuestCallOutcome::Returned(value) => value,
                        transfer => return Ok(Some(transfer)),
                    };
                    if name.ends_with("_e") && v != 0 && first_err == 0 {
                        first_err = v;
                    }
                }
                a = a.wrapping_add(4);
            }
            first_err
        }
        "_initialize_narrow_environment"
        | "_initialize_wide_environment"
        | "_configure_narrow_argv"
        | "_configure_wide_argv"
        | "_initialize_onexit_table"
        | "_register_onexit_function"
        | "_crt_atexit"
        | "_crt_at_quick_exit"
        | "_set_invalid_parameter_handler"
        | "_set_app_type"
        | "_set_fmode"
        | "_set_new_mode"
        | "setvbuf"
        | "_crt_debugger_hook"
        | "_controlfp_s"
        | "_lock"
        | "_unlock"
        | "__setusermatherr"
        | "_register_thread_local_exe_atexit_callback" => 0,
        "malloc" => state.heap.alloc(mem, state.process_heap, args[0], false)?,
        "calloc" => match args[0].checked_mul(args[1]) {
            Some(n) => state.heap.alloc(mem, state.process_heap, n, true)?,
            None => 0,
        },
        "realloc" => {
            if args[0] == 0 {
                state.heap.alloc(mem, state.process_heap, args[1], false)?
            } else if args[1] == 0 {
                state.heap.free(state.process_heap, args[0]);
                0
            } else {
                state
                    .heap
                    .realloc(mem, state.process_heap, args[0], args[1], 0)?
            }
        }
        "free" | "_free_dbg" => {
            state.heap.free(state.process_heap, args[0]);
            0
        }
        "memcpy" | "memmove" => {
            let n = (args[2] as usize).min(1 << 26);
            let mut tmp = vec![0u8; n];
            mem.read_bytes(args[1], &mut tmp)
                .map_err(|e| e.to_string())?;
            mem.write_bytes(args[0], &tmp).map_err(|e| e.to_string())?;
            args[0]
        }
        "memset" => {
            let n = (args[2] as usize).min(1 << 26);
            let tmp = vec![args[1] as u8; n];
            mem.write_bytes(args[0], &tmp).map_err(|e| e.to_string())?;
            args[0]
        }
        "memcmp" => {
            if args[2] == 0 {
                0
            } else {
                if args[2] > 1 << 26 {
                    return Err("memcmp range too large".into());
                }
                let left = mem
                    .ptr(args[0], args[2], false)
                    .map_err(|e| e.to_string())?;
                let right = mem
                    .ptr(args[1], args[2], false)
                    .map_err(|e| e.to_string())?;
                // Both complete ranges are checked, and no guest callback or
                // mutation can invalidate the backing during comparison.
                let (left, right) = unsafe {
                    (
                        std::slice::from_raw_parts(left, args[2] as usize),
                        std::slice::from_raw_parts(right, args[2] as usize),
                    )
                };
                left.iter()
                    .zip(right)
                    .find_map(|(&a, &b)| (a != b).then_some((a as i32 - b as i32) as u32))
                    .unwrap_or(0)
            }
        }
        "toupper" | "tolower" => {
            // The current CRT supports the default C locale. Values outside
            // ASCII (including EOF) retain their exact integer value.
            match (name, args[0]) {
                ("toupper", 0x61..=0x7a) => args[0] - 32,
                ("tolower", 0x41..=0x5a) => args[0] + 32,
                _ => args[0],
            }
        }
        "strcmp" | "strncmp" => {
            let limit = if name == "strncmp" { args[2] } else { u32::MAX };
            let mut result = 0;
            for i in 0..limit {
                let left = mem
                    .read_u8(args[0].checked_add(i).ok_or("strcmp address overflow")?)
                    .map_err(|e| e.to_string())?;
                let right = mem
                    .read_u8(args[1].checked_add(i).ok_or("strcmp address overflow")?)
                    .map_err(|e| e.to_string())?;
                if left != right {
                    result = (left as i32 - right as i32) as u32;
                    break;
                }
                if left == 0 {
                    break;
                }
            }
            result
        }
        "_strdup" | "strdup" => {
            let mut bytes = Vec::new();
            for i in 0..1 << 20 {
                let b = mem
                    .read_u8(args[0].checked_add(i).ok_or("strdup address overflow")?)
                    .map_err(|e| e.to_string())?;
                bytes.push(b);
                if b == 0 {
                    break;
                }
            }
            if bytes.last() != Some(&0) {
                return Err("unterminated strdup source".into());
            }
            let p = state
                .heap
                .alloc(mem, state.process_heap, bytes.len() as u32, false)?;
            if p != 0 {
                mem.write_bytes(p, &bytes).map_err(|e| e.to_string())?;
            }
            p
        }
        "strtoul" => {
            let mut bytes = Vec::new();
            let mut terminated = false;
            for i in 0..1 << 20 {
                let b = mem
                    .read_u8(args[0].checked_add(i).ok_or("strtoul address overflow")?)
                    .map_err(|e| e.to_string())?;
                if b == 0 {
                    terminated = true;
                    break;
                }
                bytes.push(b);
            }
            if !terminated {
                return Err("unterminated strtoul source".into());
            }
            let digit = |b: u8| match b {
                b'0'..=b'9' => Some((b - b'0') as u32),
                b'a'..=b'z' => Some((b - b'a' + 10) as u32),
                b'A'..=b'Z' => Some((b - b'A' + 10) as u32),
                _ => None,
            };
            let mut pos = 0;
            while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
                pos += 1;
            }
            let negative = bytes.get(pos) == Some(&b'-');
            if bytes.get(pos).is_some_and(|b| matches!(b, b'+' | b'-')) {
                pos += 1;
            }
            let mut base = args[2];
            let prefix = bytes.get(pos) == Some(&b'0')
                && bytes.get(pos + 1).is_some_and(|b| matches!(b, b'x' | b'X'))
                && bytes
                    .get(pos + 2)
                    .and_then(|&b| digit(b))
                    .is_some_and(|n| n < 16);
            if (base == 0 || base == 16) && prefix {
                base = 16;
                pos += 2;
            }
            if base == 0 {
                base = if bytes.get(pos) == Some(&b'0') { 8 } else { 10 };
            }
            let start = pos;
            let mut n = 0u32;
            let mut overflow = false;
            if (2..=36).contains(&base) {
                while let Some(d) = bytes.get(pos).and_then(|&b| digit(b)).filter(|&d| d < base) {
                    if let Some(value) = n.checked_mul(base).and_then(|v| v.checked_add(d)) {
                        n = value;
                    } else {
                        overflow = true;
                    }
                    pos += 1;
                }
            }
            if args[1] != 0 {
                mem.write_u32(
                    args[1],
                    args[0]
                        .checked_add(if pos == start { 0 } else { pos as u32 })
                        .ok_or("strtoul end pointer overflow")?,
                )
                .map_err(|e| e.to_string())?;
            }
            if overflow || !(2..=36).contains(&base) {
                let p = state.errno_address(mem, sched.current_tid())?;
                mem.write_u32(p, if overflow { 34 } else { 22 })
                    .map_err(|e| e.to_string())?;
            }
            if overflow {
                u32::MAX
            } else if negative {
                n.wrapping_neg()
            } else {
                n
            }
        }
        "strlen" => {
            let mut len = None;
            for i in 0..1 << 20 {
                if mem
                    .read_u8(args[0].checked_add(i).ok_or("strlen address overflow")?)
                    .map_err(|e| e.to_string())?
                    == 0
                {
                    len = Some(i);
                    break;
                }
            }
            len.ok_or("unterminated strlen source")?
        }
        "exit" | "_exit" | "_cexit" | "quick_exit" => {
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            0
        }
        _ => return Ok(None),
    };
    Ok(Some(crate::cpu::GuestCallOutcome::Returned(r)))
}

#[cfg(test)]
mod tests {

    #[test]
    fn target_static_imports_have_known_stack_cleanup() {
        let mut dll = String::new();
        let mut count = 0;
        for line in include_str!("../docs/m0-imports.txt").lines() {
            if line.starts_with("  ") && !line.starts_with("    ") {
                if let Some((name, _)) = line.trim().split_once(" (") {
                    if name.to_ascii_lowercase().ends_with(".dll") {
                        dll = name.to_ascii_lowercase();
                    }
                }
            } else if line.starts_with("    ") && !dll.is_empty() {
                for name in line.split_whitespace() {
                    assert!(
                        super::stack_args_bytes(&dll, name).is_some(),
                        "missing cleanup for {dll}!{name}"
                    );
                    count += 1;
                }
            }
        }
        assert_eq!(count, 346);
    }
}
