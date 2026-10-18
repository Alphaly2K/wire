//! Export identity and ABI plans are resolved during binding, not execution.
//! Unconverted families remain on explicit legacy routes.
mod address_waits;
mod basic_strings;
mod bstr;
mod calendar;
mod classes;
mod clocks;
mod com;
mod context;
mod critical_sections;
mod cursor;
mod delays;
mod desktop;
mod display;
mod files;
mod gdi_dc;
mod geometry;
mod guid;
mod handles;
mod heaps;
mod identity;
mod images;
mod inline_sync;
mod input_profile;
mod interlocked;
mod keyboard;
mod local_memory;
mod locals;
mod modules;
mod multimedia;
mod mutexes;
mod nls;
mod paths;
mod pointer_helpers;
mod process_queries;
mod sync_objects;
mod system_profile;
mod thread_control;
mod thread_properties;
mod timers;
mod user_objects;
mod variant;
mod version;
mod virtual_memory;
mod waits;
mod window_control;
mod window_queries;
mod window_state;
mod winrt;
pub use address_waits::{AddressWaitApi, ADDRESS_WAIT_CONTRACTS};
pub use basic_strings::{BasicStringApi, BASIC_STRING_CONTRACTS};
pub(crate) use bstr::symbol as bstr_symbol;
pub use bstr::{BstrApi, BSTR_CONTRACTS};
pub use calendar::{CalendarApi, CALENDAR_CONTRACTS};
pub use classes::{ClassApi, CLASS_CONTRACTS};
pub use clocks::{ClockApi, CLOCK_CONTRACTS};
pub use com::{ComApi, COM_CONTRACTS};
pub use context::{ContextApi, CONTEXT_CONTRACTS};
pub use critical_sections::{CriticalSectionApi, CRITICAL_SECTION_CONTRACTS};
pub use cursor::{CursorApi, CURSOR_CONTRACTS};
pub use delays::{DelayApi, DELAY_CONTRACTS};
pub use desktop::{DesktopApi, DESKTOP_CONTRACTS};
pub use display::{DisplayApi, DISPLAY_CONTRACTS};
pub use files::{FileApi, FILE_CONTRACTS};
pub use gdi_dc::{GdiDcApi, GDI_DC_CONTRACTS};
pub use geometry::{GeometryApi, GEOMETRY_CONTRACTS};
pub use guid::{GuidApi, GUID_CONTRACTS};
pub use handles::{HandleApi, HANDLE_CONTRACTS};
pub use heaps::{HeapApi, HEAP_CONTRACTS};
pub use identity::{IdentityApi, IDENTITY_CONTRACTS};
pub use images::{ImageApi, IMAGE_CONTRACTS};
pub use inline_sync::{InlineSyncApi, INLINE_SYNC_CONTRACTS};
pub use input_profile::{ImeApi, RawInputApi, IME_CONTRACTS, RAW_INPUT_CONTRACTS};
pub use interlocked::{InterlockedApi, INTERLOCKED_CONTRACTS};
pub use keyboard::{KeyboardApi, KEYBOARD_CONTRACTS};
pub use local_memory::{LocalMemoryApi, LOCAL_MEMORY_CONTRACTS};
pub use locals::{LocalApi, LOCAL_CONTRACTS};
pub use modules::{ModuleApi, ModuleQueryApi, MODULE_CONTRACTS, MODULE_QUERY_CONTRACTS};
pub use multimedia::{MultimediaApi, MULTIMEDIA_CONTRACTS};
pub use mutexes::{MutexApi, MUTEX_CONTRACTS};
pub use nls::{NlsApi, NLS_CONTRACTS};
pub use paths::{PathApi, PATH_CONTRACTS};
pub use pointer_helpers::{PointerHelperApi, POINTER_HELPER_CONTRACTS};
pub use process_queries::{ProcessQueryApi, PROCESS_QUERY_CONTRACTS};
pub use sync_objects::{SyncObjectApi, SYNC_OBJECT_CONTRACTS};
pub use system_profile::{SystemProfileApi, SYSTEM_PROFILE_CONTRACTS};
pub use thread_control::{ThreadControlApi, THREAD_CONTROL_CONTRACTS};
pub use thread_properties::{ThreadPropertyApi, THREAD_PROPERTY_CONTRACTS};
pub use timers::{TimerApi, TIMER_CONTRACTS};
pub use user_objects::{UserObjectApi, USER_OBJECT_CONTRACTS};
pub use variant::{VariantApi, VARIANT_CONTRACTS};
pub use version::{VersionApi, VERSION_CONTRACTS};
pub use virtual_memory::{VirtualMemoryApi, VIRTUAL_MEMORY_CONTRACTS};
pub use waits::{WaitApi, WAIT_CONTRACTS};
pub use window_control::{WindowControlApi, WINDOW_CONTROL_CONTRACTS};
pub use window_queries::{WindowQueryApi, WINDOW_QUERY_CONTRACTS};
pub use window_state::{WindowStateApi, WINDOW_STATE_CONTRACTS};
pub use winrt::{WinrtApi, WINRT_CONTRACTS};

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
    Com(ComApi),
    Winrt(WinrtApi),
    Bstr(BstrApi),
    Variant(VariantApi),
    Guid(GuidApi),
    Identity(IdentityApi),
    Version(VersionApi),
    Nls(NlsApi),
    Module(ModuleApi),
    ModuleQuery(ModuleQueryApi),
    File(FileApi),
    Interlocked(InterlockedApi),
    Handle(HandleApi),
    Path(PathApi),
    Clock(ClockApi),
    Context(ContextApi),
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

#[derive(Debug)]
pub struct Contract {
    pub id: ApiId,
    pub name: &'static str,
    pub parameters: &'static [Parameter],
    pub returns: ReturnConvention,
    pub limits: &'static str,
    plans: [ArchitecturePlan; 2],
    lookup_dlls: Option<[&'static [&'static str]; 2]>,
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
        assert!(parameters.len() <= 16, "scalar gateway argument limit");
        let mut dwords = [0; 16];
        let mut dword_count = 0;
        let mut i = 0;
        while i < parameters.len() {
            if matches!(parameters[i], Parameter::Int32 | Parameter::Uint32) {
                dwords[dword_count] = i;
                dword_count += 1;
            }
            i += 1;
        }
        Self {
            id,
            name,
            parameters,
            returns,
            limits,
            plans,
            lookup_dlls: None,
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
    pub fn execution(&self, architecture: Architecture) -> ExecutionPolicy {
        self.plans[match architecture {
            Architecture::X86 => 0,
            Architecture::X64 => 1,
        }]
        .execution
    }
    pub fn stdcall_bytes(&self) -> u32 {
        self.parameters
            .iter()
            .map(|parameter| {
                if matches!(parameter, Parameter::Int64 | Parameter::Uint64) {
                    8
                } else {
                    4
                }
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
    if contract
        .dlls(architecture)
        .iter()
        .any(|owner| dll.eq_ignore_ascii_case(owner))
    {
        Resolution::Bound(contract)
    } else {
        Resolution::WrongDll(contract)
    }
}

pub(crate) fn by_name(name: &str) -> Option<&'static Contract> {
    contracts().find(|contract| contract.name == name)
}

pub fn contracts() -> impl Iterator<Item = &'static Contract> {
    GUID_CONTRACTS
        .iter()
        .chain(IDENTITY_CONTRACTS.iter())
        .chain(VERSION_CONTRACTS.iter())
        .chain(NLS_CONTRACTS.iter())
        .chain(MODULE_CONTRACTS.iter())
        .chain(MODULE_QUERY_CONTRACTS.iter())
        .chain(FILE_CONTRACTS.iter())
        .chain(INTERLOCKED_CONTRACTS.iter())
        .chain(HANDLE_CONTRACTS.iter())
        .chain(PATH_CONTRACTS.iter())
        .chain(CLOCK_CONTRACTS.iter())
        .chain(CONTEXT_CONTRACTS.iter())
        .chain(LOCAL_CONTRACTS.iter())
        .chain(SYNC_OBJECT_CONTRACTS.iter())
        .chain(MUTEX_CONTRACTS.iter())
        .chain(WAIT_CONTRACTS.iter())
        .chain(DELAY_CONTRACTS.iter())
        .chain(TIMER_CONTRACTS.iter())
        .chain(VIRTUAL_MEMORY_CONTRACTS.iter())
        .chain(HEAP_CONTRACTS.iter())
        .chain(LOCAL_MEMORY_CONTRACTS.iter())
        .chain(SYSTEM_PROFILE_CONTRACTS.iter())
        .chain(CALENDAR_CONTRACTS.iter())
        .chain(MULTIMEDIA_CONTRACTS.iter())
        .chain(INLINE_SYNC_CONTRACTS.iter())
        .chain(CRITICAL_SECTION_CONTRACTS.iter())
        .chain(ADDRESS_WAIT_CONTRACTS.iter())
        .chain(THREAD_PROPERTY_CONTRACTS.iter())
        .chain(THREAD_CONTROL_CONTRACTS.iter())
        .chain(COM_CONTRACTS.iter())
        .chain(WINRT_CONTRACTS.iter())
        .chain(BSTR_CONTRACTS.iter())
        .chain(VARIANT_CONTRACTS.iter())
        .chain(BASIC_STRING_CONTRACTS.iter())
        .chain(POINTER_HELPER_CONTRACTS.iter())
        .chain(PROCESS_QUERY_CONTRACTS.iter())
        .chain(KEYBOARD_CONTRACTS.iter())
        .chain(RAW_INPUT_CONTRACTS.iter())
        .chain(IME_CONTRACTS.iter())
        .chain(GEOMETRY_CONTRACTS.iter())
        .chain(CURSOR_CONTRACTS.iter())
        .chain(IMAGE_CONTRACTS.iter())
        .chain(DISPLAY_CONTRACTS.iter())
        .chain(GDI_DC_CONTRACTS.iter())
        .chain(DESKTOP_CONTRACTS.iter())
        .chain(CLASS_CONTRACTS.iter())
        .chain(USER_OBJECT_CONTRACTS.iter())
        .chain(WINDOW_QUERY_CONTRACTS.iter())
        .chain(WINDOW_CONTROL_CONTRACTS.iter())
        .chain(WINDOW_STATE_CONTRACTS.iter())
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
            assert!(contract
                .win64_dwords()
                .iter()
                .all(|&i| i < contract.parameters.len()));
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
