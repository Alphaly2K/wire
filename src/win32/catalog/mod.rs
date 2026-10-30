//! Export identity and ABI plans are resolved during binding, not execution.
//! Unconverted families remain on explicit legacy routes.
mod accessibility;
mod address_waits;
mod audio;
mod basic_strings;
mod bcrypt;
mod bstr;
mod calendar;
mod certificates;
mod classes;
mod clocks;
mod com;
mod console;
mod context;
mod critical_sections;
mod crypto;
mod cursor;
mod debug;
mod delays;
mod desktop;
mod devices;
mod dialog;
mod display;
mod environment;
mod exceptions;
mod files;
mod font_resources;
mod gdi_dc;
mod gdi_operations;
mod geometry;
mod graphics;
mod graphics_probe;
mod guid;
mod handles;
mod heaps;
mod icmp;
mod identity;
mod images;
mod inline_sync;
mod input_profile;
mod interlocked;
mod keyboard;
mod local_memory;
mod locals;
mod menu;
mod message_format;
mod messages;
mod modules;
mod multimedia;
mod mutexes;
mod nls;
mod painting;
mod paths;
mod pointer_helpers;
mod process_control;
mod process_memory;
mod process_options;
mod process_queries;
mod process_startup;
mod registry;
mod resources;
mod shell;
mod sids;
mod snapshots;
mod sspi;
mod sync_objects;
mod system_paths;
mod system_profile;
mod thread_control;
mod thread_properties;
mod timers;
mod tokens;
mod tracing;
mod user_objects;
mod variant;
mod version;
mod version_resources;
mod virtual_memory;
mod waits;
mod window_control;
mod window_queries;
mod window_state;
mod windows_printf;
mod winhttp;
mod winrt;
mod winsock;
pub use accessibility::{AccessibilityApi, ACCESSIBILITY_CONTRACTS};
pub use address_waits::{AddressWaitApi, ADDRESS_WAIT_CONTRACTS};
pub use audio::{AudioApi, AUDIO_CONTRACTS};
pub use basic_strings::{BasicStringApi, BASIC_STRING_CONTRACTS};
pub use bcrypt::{BcryptApi, BCRYPT_CONTRACTS};
pub(crate) use bstr::symbol as bstr_symbol;
pub use bstr::{BstrApi, BSTR_CONTRACTS};
pub use calendar::{CalendarApi, CALENDAR_CONTRACTS};
pub use certificates::{CertificateApi, CERTIFICATE_CONTRACTS};
pub use classes::{ClassApi, CLASS_CONTRACTS};
pub use clocks::{ClockApi, CLOCK_CONTRACTS};
pub use com::{ComApi, COM_CONTRACTS};
pub use console::{ConsoleApi, CONSOLE_CONTRACTS};
pub use context::{ContextApi, CONTEXT_CONTRACTS};
pub use critical_sections::{CriticalSectionApi, CRITICAL_SECTION_CONTRACTS};
pub use crypto::{CryptoApi, CRYPTO_CONTRACTS};
pub use cursor::{CursorApi, CURSOR_CONTRACTS};
pub use debug::{DebugApi, DEBUG_CONTRACTS};
pub use delays::{DelayApi, DELAY_CONTRACTS};
pub use desktop::{DesktopApi, DESKTOP_CONTRACTS};
pub use devices::{DeviceApi, DEVICE_CONTRACTS};
pub use dialog::{DialogApi, DIALOG_CONTRACTS};
pub use display::{DisplayApi, DISPLAY_CONTRACTS};
pub use environment::{EnvironmentApi, ENVIRONMENT_CONTRACTS};
pub use exceptions::{ExceptionApi, EXCEPTION_CONTRACTS};
pub use files::{FileApi, FILE_CONTRACTS};
pub use font_resources::{FontResourceApi, FONT_RESOURCE_CONTRACTS};
pub use gdi_dc::{GdiDcApi, GDI_DC_CONTRACTS};
pub use gdi_operations::{GdiApi, GDI_OPERATION_CONTRACTS};
pub use geometry::{GeometryApi, GEOMETRY_CONTRACTS};
pub(crate) use graphics::symbol as graphics_symbol;
pub use graphics::{GraphicsExportApi, GRAPHICS_EXPORT_CONTRACTS};
pub use graphics_probe::{GraphicsProbeApi, GRAPHICS_PROBE_CONTRACTS};
pub use guid::{GuidApi, GUID_CONTRACTS};
pub use handles::{HandleApi, HANDLE_CONTRACTS};
pub use heaps::{HeapApi, HEAP_CONTRACTS};
pub use icmp::{IcmpApi, ICMP_CONTRACTS};
pub use identity::{IdentityApi, IDENTITY_CONTRACTS};
pub use images::{ImageApi, IMAGE_CONTRACTS};
pub use inline_sync::{InlineSyncApi, INLINE_SYNC_CONTRACTS};
pub use input_profile::{ImeApi, RawInputApi, IME_CONTRACTS, RAW_INPUT_CONTRACTS};
pub use interlocked::{InterlockedApi, INTERLOCKED_CONTRACTS};
pub use keyboard::{KeyboardApi, KEYBOARD_CONTRACTS};
pub use local_memory::{LocalMemoryApi, LOCAL_MEMORY_CONTRACTS};
pub use locals::{LocalApi, LOCAL_CONTRACTS};
pub use menu::{MenuApi, MENU_CONTRACTS};
pub use message_format::{MessageFormatApi, MESSAGE_FORMAT_CONTRACTS};
pub use messages::{MessageApi, MESSAGE_CONTRACTS};
pub use modules::{ModuleApi, ModuleQueryApi, MODULE_CONTRACTS, MODULE_QUERY_CONTRACTS};
pub use multimedia::{MultimediaApi, MULTIMEDIA_CONTRACTS};
pub use mutexes::{MutexApi, MUTEX_CONTRACTS};
pub use nls::{NlsApi, NLS_CONTRACTS};
pub use painting::{PaintingApi, PAINTING_CONTRACTS};
pub use paths::{PathApi, PATH_CONTRACTS};
pub use pointer_helpers::{PointerHelperApi, POINTER_HELPER_CONTRACTS};
pub use process_control::{ProcessControlApi, PROCESS_CONTROL_CONTRACTS};
pub use process_memory::{ProcessMemoryApi, PROCESS_MEMORY_CONTRACTS};
pub use process_options::{ProcessOptionApi, PROCESS_OPTION_CONTRACTS};
pub use process_queries::{ProcessQueryApi, PROCESS_QUERY_CONTRACTS};
pub use process_startup::{StartupApi, STARTUP_CONTRACTS};
pub use registry::{RegistryApi, REGISTRY_CONTRACTS};
pub use resources::{ResourceApi, RESOURCE_CONTRACTS};
pub use shell::{ShellApi, SHELL_CONTRACTS};
pub use sids::{SidApi, SID_CONTRACTS};
pub use snapshots::{SnapshotApi, SNAPSHOT_CONTRACTS};
pub use sspi::{SspiApi, SSPI_CONTRACTS};
pub use sync_objects::{SyncObjectApi, SYNC_OBJECT_CONTRACTS};
pub use system_paths::{SystemPathApi, SYSTEM_PATH_CONTRACTS};
pub use system_profile::{SystemProfileApi, SYSTEM_PROFILE_CONTRACTS};
pub use thread_control::{ThreadControlApi, THREAD_CONTROL_CONTRACTS};
pub use thread_properties::{ThreadPropertyApi, THREAD_PROPERTY_CONTRACTS};
pub use timers::{TimerApi, TIMER_CONTRACTS};
pub use tokens::{TokenApi, TOKEN_CONTRACTS};
pub use tracing::{EtwApi, ETW_CONTRACTS};
pub use user_objects::{UserObjectApi, USER_OBJECT_CONTRACTS};
pub use variant::{VariantApi, VARIANT_CONTRACTS};
mod safe_arrays;
mod security;
pub use safe_arrays::{SafeArrayApi, SAFE_ARRAY_CONTRACTS};
pub use security::{SecurityApi, SECURITY_CONTRACTS};
pub use version::{VersionApi, VERSION_CONTRACTS};
pub use version_resources::{VersionResourceApi, VERSION_RESOURCE_CONTRACTS};
pub use virtual_memory::{VirtualMemoryApi, VIRTUAL_MEMORY_CONTRACTS};
pub use waits::{WaitApi, WAIT_CONTRACTS};
pub use window_control::{WindowControlApi, WINDOW_CONTROL_CONTRACTS};
pub use window_queries::{WindowQueryApi, WINDOW_QUERY_CONTRACTS};
pub use window_state::{WindowStateApi, WINDOW_STATE_CONTRACTS};
pub use windows_printf::{WindowsPrintfApi, WINDOWS_PRINTF_CONTRACTS};
pub use winhttp::{WinHttpApi, WINHTTP_CONTRACTS};
pub use winrt::{WinrtApi, WINRT_CONTRACTS};
pub(crate) use winsock::{ordinal as winsock_ordinal, symbol as winsock_symbol};
pub use winsock::{WinsockApi, WINSOCK_CONTRACTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture {
    X86,
    X64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    Pointer,
    Int32,
    Uint32,
    /// A byte is consumed by its adapter; retain the historical Win64 carrier.
    Uint8,
    Uint64,
    Int64,
    /// A WORD is consumed by its adapter; retain the historical Win64 carrier.
    Uint16,
    /// By-value 16/24-byte VARIANT; captured in three u64 slots on both ABIs.
    Variant,
}

impl Parameter {
    pub const fn slots(self) -> usize {
        if matches!(self, Self::Variant) {
            3
        } else {
            1
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnConvention {
    Pointer,
    CharacterCount,
    Hresult,
    BoolLastError,
    Uint32,
    Uint64,
    Ntstatus,
    /// No ABI result; the adapter preserves its historical trace carrier.
    Void,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiId {
    GraphicsProbe(GraphicsProbeApi),
    Crypto(CryptoApi),
    WindowsPrintf(WindowsPrintfApi),
    Audio(AudioApi),
    FontResource(FontResourceApi),
    GraphicsExport(GraphicsExportApi),
    Exception(ExceptionApi),
    MessageFormat(MessageFormatApi),
    Shell(ShellApi),
    ProcessControl(ProcessControlApi),
    Com(ComApi),
    Winrt(WinrtApi),
    Bstr(BstrApi),
    Variant(VariantApi),
    SafeArray(SafeArrayApi),
    Guid(GuidApi),
    Identity(IdentityApi),
    Version(VersionApi),
    VersionResource(VersionResourceApi),
    Nls(NlsApi),
    Module(ModuleApi),
    ModuleQuery(ModuleQueryApi),
    File(FileApi),
    Interlocked(InterlockedApi),
    Handle(HandleApi),
    Path(PathApi),
    Clock(ClockApi),
    Context(ContextApi),
    Debug(DebugApi),
    Local(LocalApi),
    SyncObject(SyncObjectApi),
    Mutex(MutexApi),
    Wait(WaitApi),
    Delay(DelayApi),
    Timer(TimerApi),
    VirtualMemory(VirtualMemoryApi),
    Heap(HeapApi),
    LocalMemory(LocalMemoryApi),
    SystemProfile(SystemProfileApi),
    Calendar(CalendarApi),
    Multimedia(MultimediaApi),
    InlineSync(InlineSyncApi),
    CriticalSection(CriticalSectionApi),
    AddressWait(AddressWaitApi),
    ThreadProperty(ThreadPropertyApi),
    ThreadControl(ThreadControlApi),
    BasicString(BasicStringApi),
    PointerHelper(PointerHelperApi),
    ProcessQuery(ProcessQueryApi),
    ProcessMemory(ProcessMemoryApi),
    Keyboard(KeyboardApi),
    RawInput(RawInputApi),
    Ime(ImeApi),
    Geometry(GeometryApi),
    Cursor(CursorApi),
    Image(ImageApi),
    Display(DisplayApi),
    GdiDc(GdiDcApi),
    Desktop(DesktopApi),
    Class(ClassApi),
    UserObject(UserObjectApi),
    WindowQuery(WindowQueryApi),
    WindowControl(WindowControlApi),
    WindowState(WindowStateApi),
    Message(MessageApi),
    Dialog(DialogApi),
    Painting(PaintingApi),
    Menu(MenuApi),
    GdiOperation(GdiApi),
    Resource(ResourceApi),
    Startup(StartupApi),
    Environment(EnvironmentApi),
    Snapshot(SnapshotApi),
    Token(TokenApi),
    Sid(SidApi),
    Etw(EtwApi),
    Registry(RegistryApi),
    Bcrypt(BcryptApi),
    Sspi(SspiApi),
    WinHttp(WinHttpApi),
    Device(DeviceApi),
    Winsock(WinsockApi),
    ProcessOption(ProcessOptionApi),
    SystemPath(SystemPathApi),
    Certificate(CertificateApi),
    Icmp(IcmpApi),
    Accessibility(AccessibilityApi),
    Console(ConsoleApi),
    Security(SecurityApi),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPolicy {
    Typed,
    /// ABI is known; preserve the PE32 unimplemented diagnostic/return policy.
    LegacyUnimplemented,
    /// No binding for this architecture; preserve its prior rejection/unknown ABI.
    Absent,
}

#[derive(Clone, Copy, Debug)]
pub struct ArchitecturePlan {
    pub dlls: &'static [&'static str],
    pub execution: ExecutionPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallingConvention32 {
    Stdcall,
    Cdecl,
}

#[derive(Debug)]
pub struct Contract {
    pub id: ApiId,
    pub name: &'static str,
    pub parameters: &'static [Parameter],
    pub returns: ReturnConvention,
    pub limits: &'static str,
    pub x86_calling_convention: CallingConvention32,
    plans: [ArchitecturePlan; 2],
    lookup_dlls: Option<[&'static [&'static str]; 2]>,
    exact_dlls: &'static [&'static str],
    dwords: [usize; 16],
    dword_count: usize,
}

impl Contract {
    pub(super) const fn new(
        id: ApiId,
        name: &'static str,
        parameters: &'static [Parameter],
        returns: ReturnConvention,
        limits: &'static str,
        plans: [ArchitecturePlan; 2],
    ) -> Self {
        let mut dwords = [0; 16];
        let mut dword_count = 0;
        let mut i = 0;
        let mut slot = 0;
        while i < parameters.len() {
            assert!(slot + parameters[i].slots() <= 16, "gateway argument limit");
            if matches!(parameters[i], Parameter::Int32 | Parameter::Uint32) {
                dwords[dword_count] = slot;
                dword_count += 1;
            }
            slot += parameters[i].slots();
            i += 1;
        }
        Self {
            id,
            name,
            parameters,
            returns,
            limits,
            x86_calling_convention: CallingConvention32::Stdcall,
            plans,
            lookup_dlls: None,
            exact_dlls: &[],
            dwords,
            dword_count,
        }
    }
    pub fn dlls(&self, architecture: Architecture) -> &'static [&'static str] {
        match architecture {
            Architecture::X86 => self.plans[0].dlls,
            Architecture::X64 => self.plans[1].dlls,
        }
    }
    pub(super) const fn with_x86_cdecl(mut self) -> Self {
        self.x86_calling_convention = CallingConvention32::Cdecl;
        self
    }
    /// Some legacy providers accept static imports that GetProcAddress rejects.
    /// Preserve that distinction explicitly without changing the import plan.
    pub(super) const fn with_lookup_dlls(mut self, dlls: [&'static [&'static str]; 2]) -> Self {
        self.lookup_dlls = Some(dlls);
        self
    }
    pub fn lookup_dlls(&self, architecture: Architecture) -> &'static [&'static str] {
        match self.lookup_dlls {
            Some(dlls) => {
                dlls[match architecture {
                    Architecture::X86 => 0,
                    Architecture::X64 => 1,
                }]
            }
            None => self.dlls(architecture),
        }
    }
    /// Internal gateway providers can retain case-sensitive identity.
    pub(super) const fn with_exact_dlls(mut self, dlls: &'static [&'static str]) -> Self {
        self.exact_dlls = dlls;
        self
    }
    pub fn execution(&self, architecture: Architecture) -> ExecutionPolicy {
        self.plans[match architecture {
            Architecture::X86 => 0,
            Architecture::X64 => 1,
        }]
        .execution
    }
    pub fn stdcall_bytes(&self) -> u32 {
        if self.x86_calling_convention == CallingConvention32::Cdecl {
            return 0;
        }
        self.parameters
            .iter()
            .map(|parameter| match parameter {
                Parameter::Variant => 16,
                Parameter::Int64 | Parameter::Uint64 => 8,
                _ => 4,
            })
            .sum()
    }
    pub fn win64_dwords(&'static self) -> &'static [usize] {
        &self.dwords[..self.dword_count]
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Resolution {
    Bound(&'static Contract),
    WrongDll(&'static Contract),
    Legacy,
}

pub fn resolve(architecture: Architecture, dll: &str, name: &str) -> Resolution {
    let Some(contract) = by_name(name) else {
        return Resolution::Legacy;
    };
    if contract.dlls(architecture).iter().any(|owner| {
        if contract.exact_dlls.contains(owner) {
            dll == *owner
        } else {
            dll.eq_ignore_ascii_case(owner)
        }
    }) {
        Resolution::Bound(contract)
    } else {
        Resolution::WrongDll(contract)
    }
}

pub(crate) fn by_name(name: &str) -> Option<&'static Contract> {
    contracts().find(|contract| contract.name == name)
}

pub fn contracts() -> impl Iterator<Item = &'static Contract> {
    // Keep the cold catalog iterator's type depth constant as families grow.
    const FAMILIES: &[&[Contract]] = &[
        &GUID_CONTRACTS,
        &IDENTITY_CONTRACTS,
        &VERSION_CONTRACTS,
        &VERSION_RESOURCE_CONTRACTS,
        &NLS_CONTRACTS,
        &MODULE_CONTRACTS,
        &MODULE_QUERY_CONTRACTS,
        &FILE_CONTRACTS,
        &INTERLOCKED_CONTRACTS,
        &HANDLE_CONTRACTS,
        &PATH_CONTRACTS,
        &CLOCK_CONTRACTS,
        &CONTEXT_CONTRACTS,
        &DEBUG_CONTRACTS,
        &LOCAL_CONTRACTS,
        &SYNC_OBJECT_CONTRACTS,
        &MUTEX_CONTRACTS,
        &WAIT_CONTRACTS,
        &DELAY_CONTRACTS,
        &TIMER_CONTRACTS,
        &VIRTUAL_MEMORY_CONTRACTS,
        &HEAP_CONTRACTS,
        &LOCAL_MEMORY_CONTRACTS,
        &SYSTEM_PROFILE_CONTRACTS,
        &CALENDAR_CONTRACTS,
        &MULTIMEDIA_CONTRACTS,
        &AUDIO_CONTRACTS,
        &FONT_RESOURCE_CONTRACTS,
        &INLINE_SYNC_CONTRACTS,
        &CRITICAL_SECTION_CONTRACTS,
        &ADDRESS_WAIT_CONTRACTS,
        &THREAD_PROPERTY_CONTRACTS,
        &THREAD_CONTROL_CONTRACTS,
        &COM_CONTRACTS,
        &WINRT_CONTRACTS,
        &BSTR_CONTRACTS,
        &VARIANT_CONTRACTS,
        &BASIC_STRING_CONTRACTS,
        &POINTER_HELPER_CONTRACTS,
        &PROCESS_QUERY_CONTRACTS,
        &PROCESS_MEMORY_CONTRACTS,
        &KEYBOARD_CONTRACTS,
        &RAW_INPUT_CONTRACTS,
        &IME_CONTRACTS,
        &GEOMETRY_CONTRACTS,
        &CURSOR_CONTRACTS,
        &IMAGE_CONTRACTS,
        &DISPLAY_CONTRACTS,
        &GDI_DC_CONTRACTS,
        &DESKTOP_CONTRACTS,
        &CLASS_CONTRACTS,
        &USER_OBJECT_CONTRACTS,
        &WINDOW_QUERY_CONTRACTS,
        &WINDOW_CONTROL_CONTRACTS,
        &WINDOW_STATE_CONTRACTS,
        &MESSAGE_CONTRACTS,
        &DIALOG_CONTRACTS,
        &PAINTING_CONTRACTS,
        &MENU_CONTRACTS,
        &GDI_OPERATION_CONTRACTS,
        &RESOURCE_CONTRACTS,
        &STARTUP_CONTRACTS,
        &ENVIRONMENT_CONTRACTS,
        &SNAPSHOT_CONTRACTS,
        &TOKEN_CONTRACTS,
        &SID_CONTRACTS,
        &ETW_CONTRACTS,
        &REGISTRY_CONTRACTS,
        &BCRYPT_CONTRACTS,
        &CRYPTO_CONTRACTS,
        &SSPI_CONTRACTS,
        &WINHTTP_CONTRACTS,
        &DEVICE_CONTRACTS,
        &WINSOCK_CONTRACTS,
        &PROCESS_OPTION_CONTRACTS,
        &SYSTEM_PATH_CONTRACTS,
        &EXCEPTION_CONTRACTS,
        &MESSAGE_FORMAT_CONTRACTS,
        &SHELL_CONTRACTS,
        &PROCESS_CONTROL_CONTRACTS,
        &GRAPHICS_EXPORT_CONTRACTS,
        &GRAPHICS_PROBE_CONTRACTS,
        &WINDOWS_PRINTF_CONTRACTS,
        &SAFE_ARRAY_CONTRACTS,
        &CERTIFICATE_CONTRACTS,
        &ICMP_CONTRACTS,
        &ACCESSIBILITY_CONTRACTS,
        &CONSOLE_CONTRACTS,
        &SECURITY_CONTRACTS,
    ];
    FAMILIES.iter().flat_map(|family| family.iter())
}

/// Normalize provider-owned aliases identically in every Win64 loader.
pub(crate) fn normalize_symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    safe_arrays::symbol(
        dll,
        bstr_symbol(dll, graphics_symbol(dll, winsock_symbol(dll, name))),
    )
}

pub(crate) fn safe_array_symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    safe_arrays::symbol(dll, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations_have_unique_exports_and_valid_architecture_plans() {
        let mut names = std::collections::HashSet::new();
        for contract in contracts() {
            assert!(
                names.insert(contract.name),
                "duplicate export {}",
                contract.name
            );
            assert!(!contract.limits.is_empty());
            for dll in contract.exact_dlls {
                assert!([Architecture::X86, Architecture::X64]
                    .iter()
                    .any(|&architecture| contract.dlls(architecture).contains(dll)));
            }
            let slots = contract.parameters.iter().map(|p| p.slots()).sum();
            assert!(contract.win64_dwords().iter().all(|&i| i < slots));
            for architecture in [Architecture::X86, Architecture::X64] {
                let mut owners = std::collections::HashSet::new();
                for dll in contract.dlls(architecture) {
                    assert_eq!(*dll, dll.to_ascii_lowercase());
                    assert!(owners.insert(*dll), "duplicate owner {}", dll);
                }
                assert_eq!(
                    owners.is_empty(),
                    contract.execution(architecture) == ExecutionPolicy::Absent
                );
                let mut lookups = std::collections::HashSet::new();
                for dll in contract.lookup_dlls(architecture) {
                    assert!(
                        owners.contains(dll),
                        "lookup owner must have an import plan"
                    );
                    assert!(lookups.insert(*dll), "duplicate lookup owner {}", dll);
                }
            }
        }
    }
}
