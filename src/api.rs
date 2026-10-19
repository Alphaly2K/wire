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
use crate::thread::{Sched, ThreadState, WaitTarget, Wake, WAIT_OBJECT_0, WAIT_TIMEOUT};
use crate::win32::x86::object_waits::{obj_acquire, obj_ready};
use std::collections::HashMap;
pub type Args = [u32; 16];
use std::sync::Arc;
use std::time::Instant;

pub use crate::runtime::api_log::ApiLogger;
pub use crate::win32::kernel::objects32::{HandleObj, KernelObjects32};
pub use crate::win32::x86::scratch::SCRATCH_BASE;
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
            .or_else(|| ordinal.and_then(|o| crate::win32::abi::ordinal_name(&dll, o)))
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
    scratch: crate::win32::x86::scratch::Scratch32,
    crt: crate::win32::x86::crt_state::CrtState32,
    pub process_heap: u32,
    pub heap: crate::heap::GuestHeap,
    pub fs: Option<crate::fs::GuestFs>,
    pub(crate) locals: crate::win32::kernel::locals32::LocalSlots32,
    pub handles: KernelObjects32,
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
            scratch: crate::win32::x86::scratch::Scratch32::default(),
            crt: crate::win32::x86::crt_state::CrtState32::default(),
            process_heap: 0x7005_0000,
            heap: crate::heap::GuestHeap::new(0x7005_0000),
            fs: None,
            locals: crate::win32::kernel::locals32::LocalSlots32::default(),
            handles: KernelObjects32::default(),
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
        self.handles.alloc(obj)
    }

    /// CreateEvent/CreateMutex/CreateSemaphore share name-based dedup:
    /// creating a named object that already exists returns the existing
    /// handle and sets ERROR_ALREADY_EXISTS.
    pub fn create_named(&mut self, name: &str, make: impl FnOnce() -> HandleObj) -> (u32, bool) {
        crate::win32::x86::sync_objects::create_named(
            &mut self.handles,
            &mut self.last_error,
            name,
            make,
        )
    }

    pub fn module_handle_for(&mut self, dll: &str) -> u32 {
        self.modules.provider_handle_for(dll)
    }

    /// Bump-allocate guest scratch memory (for fake CRT structures).
    pub fn scratch_alloc(&mut self, mem: &mut GuestMem, size: u32, align: u32) -> u32 {
        self.scratch.alloc(mem, size, align, &mut self.last_error)
    }
    pub fn scratch_position(&self) -> u32 {
        self.scratch.position()
    }
    pub fn crt_app_type(&self) -> u32 {
        self.crt.app_type()
    }
    fn errno_address(&mut self, mem: &mut GuestMem, tid: u32) -> Result<u32, String> {
        self.crt
            .errno_address(mem, tid, &mut self.scratch, &mut self.last_error)
    }

    /// Lazy argc/argv/envp: argc=1, argv=[cmdline, NULL], envp=[NULL].
    pub fn build_argv(&mut self, mem: &mut GuestMem) -> (u32, u32, u32) {
        self.crt
            .build_argv(mem, self.cmdline_a, &mut self.scratch, &mut self.last_error)
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
    crate::win32::abi::stack_bytes(dll, name)
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
        (
            Some(crate::win32::catalog::ApiId::ProcessControl(
                crate::win32::catalog::ProcessControlApi::Terminate,
            )),
            _,
            _,
        ) => {
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
                    note = if id == ApiId::ProcessQuery(ProcessQueryApi::PackageId) {
                        "not a packaged app"
                    } else if matches!(id, ApiId::PointerHelper(id) if id.encoding()) {
                        "identity encoding"
                    } else {
                        "common process/string/COM allocation contract"
                    }
                    .into();
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
        (Some(crate::win32::catalog::ApiId::Dialog(id)), _, _) if id.message_box() => {
            (ret, note) = crate::win32::x86::dialog::message_box(id, &args, mem);
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
        (
            Some(crate::win32::catalog::ApiId::ProcessControl(
                crate::win32::catalog::ProcessControlApi::Exit,
            )),
            _,
            _,
        ) => {
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
        (Some(crate::win32::catalog::ApiId::SyncObject(id)), _, _) => {
            (ret, note) = crate::win32::x86::sync_objects::sync_call(
                id,
                &args,
                mem,
                &mut state.handles,
                sched,
                &mut state.last_error,
            )?;
        }
        (Some(crate::win32::catalog::ApiId::Mutex(id)), _, _) => {
            (ret, note) = crate::win32::x86::sync_objects::mutex_call(
                id,
                &args,
                mem,
                &mut state.handles,
                sched,
                &mut state.last_error,
            )?;
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
                    if obj_acquire(&mut state.handles, sched, h) {
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
                        if hs.iter().all(|&h| obj_ready(&state.handles, sched, h)) {
                            for &h in &hs {
                                obj_acquire(&mut state.handles, sched, h);
                            }
                            got = Some(0);
                        }
                    } else {
                        for (i, &h) in hs.iter().enumerate() {
                            if obj_acquire(&mut state.handles, sched, h) {
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
                    if input && hs.iter().all(|&h| obj_ready(&state.handles, sched, h)) {
                        for &h in &hs {
                            obj_acquire(&mut state.handles, sched, h);
                        }
                        Some(0)
                    } else {
                        None
                    }
                } else {
                    hs.iter()
                        .enumerate()
                        .find_map(|(i, &h)| {
                            obj_acquire(&mut state.handles, sched, h).then_some(i as u32)
                        })
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
            if state.handles.close(args[0]) {
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
        (
            Some(crate::win32::catalog::ApiId::Handle(
                crate::win32::catalog::HandleApi::SetHandleCount,
            )),
            _,
            _,
        ) => {
            // Legacy CRT hint; NT-style handle tables grow automatically.
            ret = args[0];
            note = "automatic handle table capacity".into();
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
        (
            Some(crate::win32::catalog::ApiId::Exception(
                crate::win32::catalog::ExceptionApi::SetFilter,
            )),
            _,
            _,
        ) => {
            ret = state.unhandled_filter;
            state.unhandled_filter = args[0];
        }
        (
            Some(crate::win32::catalog::ApiId::Exception(
                crate::win32::catalog::ExceptionApi::Raise,
            )),
            _,
            _,
        ) => {
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
        (
            Some(crate::win32::catalog::ApiId::Exception(
                crate::win32::catalog::ExceptionApi::UnhandledFilter,
            )),
            _,
            _,
        ) => {
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
        (
            Some(crate::win32::catalog::ApiId::Exception(
                crate::win32::catalog::ExceptionApi::Unwind,
            )),
            _,
            _,
        ) => {
            log.log_call(idx, dll, name, &args[..4], args[3], true, "SEH unwind");
            return crate::seh::unwind(cpu, mem, api, state, log, sched, &args, max_instr);
        }
        (Some(crate::win32::catalog::ApiId::MessageFormat(id)), _, _) => {
            (ret, note) = crate::win32::x86::message_format::call(
                id,
                &args,
                mem,
                &mut state.heap,
                state.process_heap,
                &mut state.last_error,
            )?;
        }
        (Some(crate::win32::catalog::ApiId::Shell(id)), _, _) => {
            (ret, note) = crate::win32::x86::shell::call(
                id,
                &args,
                mem,
                &mut state.heap,
                state.process_heap,
                &mut state.last_error,
            );
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
        "__p__fmode" | "__p__commode" | "__p___argc" | "__p__timezone" | "__p__daylight"
        | "__p__acmdln" | "__p__wcmdln" => api
            .data_export(mem, dll, &name[4..], state.cmdline_a, state.cmdline_w)?
            .ok_or("unknown CRT data accessor")?,
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
        "strtoul" => state.crt.strtoul(
            args,
            mem,
            sched.current_tid(),
            &mut state.scratch,
            &mut state.last_error,
        )?,
        "exit" | "_exit" | "_cexit" | "quick_exit" => {
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            0
        }
        _ => {
            if let Some(value) = crate::win32::x86::crt_memory::call(name, args, mem)? {
                value
            } else if let Some(value) = crate::win32::x86::heaps::crt_call(
                name,
                args,
                mem,
                &mut state.heap,
                state.process_heap,
            )? {
                value
            } else if let Some(value) = state.crt.startup_call(
                name,
                args,
                mem,
                state.cmdline_a,
                &mut state.scratch,
                &mut state.last_error,
            )? {
                value
            } else {
                return Ok(None);
            }
        }
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
