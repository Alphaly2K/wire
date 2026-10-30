use super::{
    bcrypt, calendar_services,
    call::Call,
    catalog::{
        self, ApiId, Architecture, ClockApi, Contract, DelayApi, ExceptionApi, ExecutionPolicy,
        MessageApi, ModuleApi, PointerHelperApi, Resolution, WindowStateApi,
    },
    com, context, crt, cursor, d3d9, desktop, directories, display, dsound, environment, etw,
    file_mutations, files, gateways, gdi, gdiplus, geometry, graphics, graphics_probe, guid, heaps,
    identity, interlocked, keyboard, locals, message_format, messages, module_queries, modules,
    nls, objects, pointer_helpers, process_control, process_create, process_memory,
    process_options, process_queries, registry, shell, sids, snapshots, startup, system_paths,
    system_profile, thread_descriptions, threads, tokens, user, version, version_resources,
    virtual_memory, wmi,
};
use crate::{
    machine::{Mode, State},
    memory::{Fault, Memory, READ, WRITE},
};

#[derive(Default)]
pub struct ServiceSample<'a> {
    pub elapsed: u64,
    pub wall: Option<u64>,
    /// Aggregate [idle, kernel, user] in 100 ns units; kernel includes idle.
    pub cpu_times: Option<Result<[u64; 3], u32>>,
    pub random: Option<&'a [u8]>,
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum ClockSource {
    None = 1,
    Elapsed = 2,
    Wall = 3,
    ElapsedWall = 4,
    Cpu = 5,
}

#[derive(Debug)]
pub enum ServiceError {
    FileInput(crate::filesystem::ReadRequest),
    Graphics,
    GraphicsFailed(String),
    Unbound,
    ImportTrap { dll: String, symbol: String },
    Fault(Fault),
    Unavailable(&'static str),
    Blocked,
    LoaderBlocked,
}

#[derive(Clone)]
enum Binding {
    Crt { export: crt::Export, dll: String },
    D3D9(d3d9::Method),
    Sound(dsound::Method),
    GdiPlus(gdiplus::Export),
    Wmi(wmi::Method),
    Graphics(graphics::BoundMethod),
    Service(&'static Contract),
    ImportTrap { dll: String, symbol: String },
}

impl Binding {
    fn contract(&self) -> Option<&'static Contract> {
        match self {
            Self::Graphics(_)
            | Self::Wmi(_)
            | Self::GdiPlus(_)
            | Self::D3D9(_)
            | Self::Sound(_)
            | Self::Crt { .. } => None,
            Self::Service(contract) => Some(contract),
            Self::ImportTrap { .. } => None,
        }
    }
}
impl From<Fault> for ServiceError {
    fn from(value: Fault) -> Self {
        Self::Fault(value)
    }
}

/// Resolved contracts keep symbol lookup outside service execution.
#[derive(Clone)]
pub struct Services {
    mode: Mode,
    bindings: Vec<Binding>,
    providers: Vec<String>,
    pub(super) crt_data: std::collections::BTreeMap<String, super::crt_startup::Data>,
    pub(super) time_zone: std::rc::Rc<super::timezone::Profile>,
}

impl graphics::dxgi::Bindings for Services {
    fn check_capacity(&self, names: &[&'static str]) -> Result<(), String> {
        if self.mode != Mode::X64
            || names
                .iter()
                .any(|name| graphics::bind_method(name).is_none_or(|b| b.arity() > 16))
        {
            return Err("invalid graphics method ABI".into());
        }
        let count = names
            .iter()
            .filter(|name| {
                !self
                    .bindings
                    .iter()
                    .any(|b| matches!(b, Binding::Graphics(m) if m.name() == **name))
            })
            .count();
        if self.address(self.bindings.len() + count) > gateways::layout(self.mode).services.end {
            return Err("Windows gateway capacity exhausted".into());
        }
        Ok(())
    }
    fn register(&mut self, dll: &str, name: &'static str) -> Result<u64, String> {
        if dll != "wire.dxgi" || self.mode != Mode::X64 {
            return Err("incorrect graphics method provider".into());
        }
        if let Some(index) = self
            .bindings
            .iter()
            .position(|b| matches!(b, Binding::Graphics(m) if m.name() == name))
        {
            return Ok(self.address(index));
        }
        self.check_capacity(&[name])?;
        self.register_provider(dll)?;
        let address = self.address(self.bindings.len());
        self.bindings.push(Binding::Graphics(
            graphics::bind_method(name).ok_or("unknown graphics method")?,
        ));
        Ok(address)
    }
}

impl Services {
    pub(super) fn check_d3d9_methods(&self, kind: &str) -> Result<(), String> {
        let count = super::d3d9_core::ABI
            .lines()
            .filter(|line| line.starts_with(&format!("{kind}.")))
            .filter(|line| {
                !self
                    .bindings
                    .iter()
                    .any(|b| matches!(b,Binding::D3D9(m) if Some(m.name)==line.split('\t').next()))
            })
            .count();
        if self.mode != Mode::X86
            || self.address(self.bindings.len() + count) > gateways::layout(self.mode).services.end
        {
            return Err("D3D9 gateway capacity exhausted".into());
        }
        Ok(())
    }
    pub(super) fn register_d3d9(&mut self, method: d3d9::Method) -> Result<u64, String> {
        if self.mode != Mode::X86
            || d3d9::Method::lookup(method.name) != Some(method)
            || method.stack > 64
        {
            return Err("invalid D3D9 method ABI".into());
        }
        if let Some(index) = self
            .bindings
            .iter()
            .position(|b| matches!(b,Binding::D3D9(m) if *m==method))
        {
            return Ok(self.address(index));
        }
        let address = self.address(self.bindings.len());
        if address >= gateways::layout(self.mode).services.end {
            return Err("Windows gateway capacity exhausted".into());
        }
        self.register_provider("d3d9.dll")?;
        self.bindings.push(Binding::D3D9(method));
        Ok(address)
    }
    pub(super) fn check_sound_methods(&self, kind: &str) -> Result<(), String> {
        let count = super::dsound_core::ABI
            .lines()
            .filter(|line| line.starts_with(&format!("{kind}.")))
            .filter(|line| {
                !self
                    .bindings
                    .iter()
                    .any(|b| matches!(b,Binding::Sound(m) if Some(m.name)==line.split('\t').next()))
            })
            .count();
        if self.mode != Mode::X86
            || self.address(self.bindings.len() + count) > gateways::layout(self.mode).services.end
        {
            return Err("DirectSound gateway capacity exhausted".into());
        }
        Ok(())
    }
    pub(super) fn register_sound(&mut self, method: dsound::Method) -> Result<u64, String> {
        if self.mode != Mode::X86
            || dsound::Method::lookup(method.name) != Some(method)
            || method.stack > 64
        {
            return Err("invalid DirectSound method ABI".into());
        }
        if let Some(index) = self
            .bindings
            .iter()
            .position(|b| matches!(b,Binding::Sound(m) if *m==method))
        {
            return Ok(self.address(index));
        }
        let address = self.address(self.bindings.len());
        if address >= gateways::layout(self.mode).services.end {
            return Err("Windows gateway capacity exhausted".into());
        }
        self.register_provider("dsound.dll")?;
        self.bindings.push(Binding::Sound(method));
        Ok(address)
    }
    pub(super) fn check_wmi_methods(&self, kind: &'static str) -> Result<(), String> {
        let entries = super::wmi_core::methods(kind).ok_or("unknown WMI interface")?;
        let count = entries
            .iter()
            .filter(|&&(name, _)| {
                !self
                    .bindings
                    .iter()
                    .any(|b| matches!(b, Binding::Wmi(m) if m.kind == kind && m.name == name))
            })
            .count();
        if self.mode != Mode::X86
            || self.address(self.bindings.len() + count) > gateways::layout(self.mode).services.end
        {
            return Err("WMI gateway capacity exhausted".into());
        }
        Ok(())
    }
    pub(super) fn register_wmi(&mut self, method: wmi::Method) -> Result<u64, String> {
        if wmi::Method::lookup(&format!("{}.{}", method.kind, method.name)) != Some(method)
            || method.stack > 64
        {
            return Err("invalid WMI method ABI".into());
        }
        if let Some(index) = self
            .bindings
            .iter()
            .position(|b| matches!(b, Binding::Wmi(m) if *m == method))
        {
            return Ok(self.address(index));
        }
        self.check_wmi_methods(method.kind)?;
        self.register_provider("wire.wmi")?;
        let address = self.address(self.bindings.len());
        self.bindings.push(Binding::Wmi(method));
        Ok(address)
    }
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            bindings: Vec::new(),
            providers: Vec::new(),
            crt_data: Default::default(),
            time_zone: std::rc::Rc::new(Default::default()),
        }
    }

    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }
    pub(crate) fn set_time_zone(&mut self, bytes: [u8; 172]) -> Result<(), String> {
        let zone = super::timezone::Profile::new(bytes)
            .map_err(|code| format!("invalid timezone profile: {code}"))?;
        zone.crt_values()
            .map_err(|_| "timezone offset exceeds CRT seconds range")?;
        self.time_zone = std::rc::Rc::new(zone);
        Ok(())
    }

    pub(crate) fn extends_provider(dll: &str, mode: Mode) -> bool {
        if super::provider_aliases::provider(dll, mode)
            || mode == Mode::X86 && crt::provider(dll).is_some()
        {
            return true;
        }
        let services = Self::new(mode);
        catalog::contracts().any(|contract| {
            services.extended_dlls(contract).contains(&dll) && Self::implemented(contract.id)
        })
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn random_request(
        &self,
        state: &State,
        memory: &Memory,
        image: Option<&crate::process::Loaded>,
    ) -> u32 {
        let id = self.contract(state.ip).map(|c| c.id);
        if !matches!(
            id,
            Some(
                ApiId::Guid(catalog::GuidApi::Create)
                    | ApiId::Bcrypt(
                        catalog::BcryptApi::Generate | catalog::BcryptApi::SystemRandom
                    )
            )
        ) {
            return 0;
        }
        let sp = self.mode.address(state.regs[4]);
        if !memory.probe(sp, self.mode as usize / 8, READ)
            || (self.mode == Mode::X64 && sp.checked_add(8).is_none())
        {
            return 0;
        }
        match id {
            Some(ApiId::Guid(catalog::GuidApi::Create)) => {
                let Some(output) = super::call::peek_slot(state, memory, 0) else {
                    return 0;
                };
                if output != 0 && memory.probe(output, 16, WRITE) {
                    0x100
                } else {
                    0
                }
            }
            Some(ApiId::Bcrypt(id)) => {
                let length = bcrypt::request(id, state, memory, image);
                if length == 0 {
                    0
                } else {
                    0x80 | (length - 1) << 8
                }
            }
            _ => 0,
        }
    }

    pub(crate) fn register_provider(&mut self, dll: &str) -> Result<u64, String> {
        let name = crate::pe::linker::module_name(dll)?;
        let existing = self.provider_handle(&name);
        if existing != 0 {
            return Ok(existing);
        }
        if self.providers.len() >= gateways::PROVIDER_CAPACITY {
            return Err("Windows provider identity capacity exceeded".into());
        }
        let handle = gateways::provider_area(self.mode).start + self.providers.len() as u64 * 65536;
        self.providers.push(name);
        Ok(handle)
    }

    pub(super) fn provider_handle(&self, dll: &str) -> u64 {
        self.providers
            .iter()
            .position(|name| name == dll)
            .map_or(0, |index| {
                gateways::provider_area(self.mode).start + index as u64 * 65536
            })
    }

    pub(super) fn providers(&self) -> impl Iterator<Item = (u64, &str)> {
        self.providers
            .iter()
            .map(|name| (self.provider_handle(name), name.as_str()))
    }

    pub(crate) fn provider_name(&self, handle: u64) -> Option<&str> {
        let offset = handle.checked_sub(gateways::provider_area(self.mode).start)?;
        if !offset.is_multiple_of(65536) {
            return None;
        }
        self.providers
            .get(usize::try_from(offset / 65536).ok()?)
            .map(String::as_str)
    }

    pub(super) fn containing_provider(&self, address: u64) -> Option<u64> {
        if let Some((dll, _)) = self
            .crt_data
            .iter()
            .find(|(_, data)| (data.base..data.base + crate::memory::PAGE).contains(&address))
        {
            return Some(self.provider_handle(dll));
        }
        if let Some(Binding::Crt { dll, .. }) = self.binding(address & !15) {
            return Some(self.provider_handle(dll));
        }
        if let Some(Binding::D3D9(_)) = self.binding(address & !15) {
            return Some(self.provider_handle("d3d9.dll"));
        }
        if let Some(Binding::Sound(_)) = self.binding(address & !15) {
            return Some(self.provider_handle("dsound.dll"));
        }
        if let Some(Binding::GdiPlus(_)) = self.binding(address & !15) {
            return Some(self.provider_handle("gdiplus.dll"));
        }
        if let Some(Binding::Wmi(_)) = self.binding(address & !15) {
            return Some(self.provider_handle("wire.wmi"));
        }
        if let Some(Binding::Graphics(_)) = self.binding(address & !15) {
            return Some(self.provider_handle("wire.dxgi"));
        }
        if let Some(Binding::ImportTrap { dll, .. }) = self.binding(address & !15) {
            return Some(self.provider_handle(dll));
        }
        let contract = self.contract(address & !15)?;
        let dlls = if self.extended(contract) {
            self.extended_dlls(contract)
        } else {
            contract.dlls(self.architecture())
        };
        dlls.iter()
            .map(|name| self.provider_handle(name))
            .find(|base| *base != 0)
            .or_else(|| {
                self.providers()
                    .find(|(_, dll)| {
                        super::provider_aliases::declares(dll, contract.name, self.mode)
                    })
                    .map(|(handle, _)| handle)
            })
    }

    fn architecture(&self) -> Architecture {
        if self.mode == Mode::X86 {
            Architecture::X86
        } else {
            Architecture::X64
        }
    }

    fn implemented(id: ApiId) -> bool {
        user::implemented(id)
            || messages::implemented(id)
            || gdi::implemented(id)
            || objects::implemented(id)
            || threads::implemented(id)
            || directories::implemented(id)
            || files::implemented(id)
            || heaps::implemented(id)
            || nls::implemented(id)
            || calendar_services::implemented(id)
            || file_mutations::implemented(id)
            || com::implemented(id)
            || matches!(id, ApiId::UserObject(id) if !id.enumeration())
            || matches!(
                id,
                ApiId::Clock(_)
                    | ApiId::Certificate(_)
                    | ApiId::Icmp(_)
                    | ApiId::Accessibility(_)
                    | ApiId::Console(_)
                    | ApiId::Security(_)
                    | ApiId::GraphicsProbe(_)
                    | ApiId::GraphicsExport(_)
                    | ApiId::ModuleQuery(_)
                    | ApiId::ProcessQuery(_)
                    | ApiId::ProcessOption(_)
                    | ApiId::ProcessMemory(_)
                    | ApiId::ProcessControl(
                        catalog::ProcessControlApi::Create
                            | catalog::ProcessControlApi::Exit
                            | catalog::ProcessControlApi::Terminate
                    )
                    | ApiId::Debug(_)
                    | ApiId::Snapshot(_)
                    | ApiId::ThreadControl(
                        catalog::ThreadControlApi::SetDescription
                            | catalog::ThreadControlApi::GetDescription
                            | catalog::ThreadControlApi::FreeAndExit
                    )
                    | ApiId::Environment(_)
                    | ApiId::Identity(_)
                    | ApiId::Version(_)
                    | ApiId::VersionResource(_)
                    | ApiId::Resource(_)
                    | ApiId::Image(_)
                    | ApiId::BasicString(_)
                    | ApiId::WindowsPrintf(_)
                    | ApiId::Audio(_)
                    | ApiId::FontResource(_)
                    | ApiId::Path(_)
                    | ApiId::Menu(_)
                    | ApiId::Registry(_)
                    | ApiId::Token(_)
                    | ApiId::Sid(_)
                    | ApiId::Guid(_)
                    | ApiId::Geometry(_)
                    | ApiId::Display(_)
                    | ApiId::Desktop(_)
                    | ApiId::SystemPath(_)
                    | ApiId::SystemProfile(_)
                    | ApiId::Startup(_)
                    | ApiId::Shell(_)
                    | ApiId::Bcrypt(_)
                    | ApiId::Crypto(_)
                    | ApiId::Etw(_)
                    | ApiId::Cursor(_)
                    | ApiId::WinHttp(_)
                    | ApiId::Winsock(_)
                    | ApiId::Bstr(_)
                    | ApiId::Variant(_)
                    | ApiId::SafeArray(_)
                    | ApiId::Winrt(_)
                    | ApiId::Ime(_)
                    | ApiId::RawInput(_)
                    | ApiId::Sspi(_)
                    | ApiId::Device(_)
                    | ApiId::Multimedia(_)
                    | ApiId::Local(_)
                    | ApiId::VirtualMemory(_)
                    | ApiId::PointerHelper(_)
                    | ApiId::Interlocked(_)
                    | ApiId::MessageFormat(_)
                    | ApiId::Context(_)
                    | ApiId::Exception(
                        ExceptionApi::AddVectored
                            | ExceptionApi::RemoveVectored
                            | ExceptionApi::SetFilter
                            | ExceptionApi::Raise
                            | ExceptionApi::AddTable
                            | ExceptionApi::DeleteTable
                            | ExceptionApi::LookupEntry
                            | ExceptionApi::UnwindEx
                            | ExceptionApi::InstallCallback
                            | ExceptionApi::CaptureContext
                            | ExceptionApi::PcToFileHeader
                            | ExceptionApi::VirtualUnwind
                            | ExceptionApi::Unwind
                            | ExceptionApi::UnhandledFilter
                    )
                    | ApiId::Module(
                        ModuleApi::GetModuleHandleA
                            | ModuleApi::GetModuleHandleW
                            | ModuleApi::GetModuleFileNameA
                            | ModuleApi::GetModuleFileNameW
                            | ModuleApi::GetProcAddress
                            | ModuleApi::LoadLibraryA
                            | ModuleApi::LoadLibraryW
                            | ModuleApi::LoadLibraryExA
                            | ModuleApi::LoadLibraryExW
                            | ModuleApi::FreeLibrary
                            | ModuleApi::GetModuleHandleExA
                            | ModuleApi::GetModuleHandleExW
                            | ModuleApi::DisableThreadLibraryCalls
                    )
            )
    }

    pub(crate) fn lookup(&mut self, dll: &str, name: &str) -> Result<u64, String> {
        if super::provider_aliases::declares(dll, name, self.mode) {
            return self.bind(dll, name);
        }
        if self.mode == Mode::X86
            && crt::provider(dll).is_some()
            && crt::Export::lookup(name).is_some()
        {
            return self.bind(dll, name);
        }
        if self.mode == Mode::X86
            && (dll.eq_ignore_ascii_case("dsound.dll") || dll.eq_ignore_ascii_case("dsound"))
        {
            return self.bind(dll, name);
        }
        if self.mode == Mode::X86
            && (dll.eq_ignore_ascii_case("gdiplus.dll") || dll.eq_ignore_ascii_case("gdiplus"))
        {
            return self.bind(dll, name);
        }
        if self.mode == Mode::X86
            && (dll.eq_ignore_ascii_case("d3d9.dll") || dll.eq_ignore_ascii_case("d3d9"))
        {
            return self.bind(dll, name);
        }
        let architecture = self.architecture();
        let contract = self.resolve(dll, name)?;
        let bound = self
            .bindings
            .iter()
            .filter_map(Binding::contract)
            .any(|c| std::ptr::eq(c, contract));
        if !bound
            && !self.extended(contract)
            && !contract
                .lookup_dlls(architecture)
                .iter()
                .any(|owner| owner.eq_ignore_ascii_case(dll))
        {
            return Err("Windows export is not visible through this provider".into());
        }
        self.bind(dll, name)
    }

    fn address(&self, index: usize) -> u64 {
        gateways::layout(self.mode).services.start
            + (index as u64 + if self.mode == Mode::X86 { 2 } else { 0 }) * 16
    }

    fn extended(&self, contract: &Contract) -> bool {
        !self.extended_dlls(contract).is_empty()
    }

    fn extended_dlls(&self, contract: &Contract) -> &'static [&'static str] {
        match contract.id {
            ApiId::Certificate(_) => return &["crypt32.dll"],
            ApiId::Security(_) => return &["advapi32.dll"],
            ApiId::Wait(catalog::WaitApi::SignalAndWait) => {
                return &["kernel32.dll", "kernelbase.dll"]
            }
            ApiId::WindowQuery(catalog::WindowQueryApi::Related) => return &["user32.dll"],
            ApiId::Icmp(_) => return &["iphlpapi.dll"],
            ApiId::Accessibility(_) => return &["uiautomationcore.dll"],
            ApiId::Console(_) => return &["kernel32.dll", "kernelbase.dll"],
            ApiId::SystemProfile(
                catalog::SystemProfileApi::LanguageNameA | catalog::SystemProfileApi::LanguageNameW,
            ) => return &["kernel32.dll", "kernelbase.dll", "version.dll"],
            ApiId::SystemProfile(
                catalog::SystemProfileApi::DiskSpaceA
                | catalog::SystemProfileApi::DiskSpaceW
                | catalog::SystemProfileApi::ProcessorNumber
                | catalog::SystemProfileApi::LogicalDrivesA
                | catalog::SystemProfileApi::LogicalDrivesW
                | catalog::SystemProfileApi::VolumeA
                | catalog::SystemProfileApi::VolumeW
                | catalog::SystemProfileApi::FlushWrites,
            ) => return &["kernel32.dll", "kernelbase.dll"],
            ApiId::GraphicsProbe(id) => return id.dlls(),
            ApiId::ProcessQuery(catalog::ProcessQueryApi::InputIdle) => return &["user32.dll"],
            ApiId::ProcessQuery(
                catalog::ProcessQueryApi::MemoryInfo | catalog::ProcessQueryApi::Enumerate,
            ) => {
                return if contract.name.starts_with("K32") {
                    &["kernel32.dll", "kernelbase.dll"]
                } else {
                    &["psapi.dll"]
                };
            }
            ApiId::Desktop(id) => {
                return match id {
                    catalog::DesktopApi::WindowAttribute
                    | catalog::DesktopApi::CompositionEnabled => &["dwmapi.dll"],
                    _ => &["user32.dll"],
                }
            }
            ApiId::Geometry(_) | ApiId::Display(_) => return &["user32.dll"],
            ApiId::WindowState(catalog::WindowStateApi::AllowForeground) => return &["user32.dll"],
            ApiId::ModuleQuery(_) => {
                return if contract.name.starts_with("K32") {
                    &["kernel32.dll", "kernelbase.dll"]
                } else {
                    &["psapi.dll"]
                }
            }
            ApiId::VersionResource(_) => return &["version.dll"],
            ApiId::Resource(id) => {
                return if matches!(
                    id,
                    catalog::ResourceApi::LoadStringA | catalog::ResourceApi::LoadStringW
                ) {
                    &["user32.dll"]
                } else {
                    &["kernel32.dll", "kernelbase.dll"]
                }
            }
            ApiId::BasicString(_) => return &["kernel32.dll", "kernelbase.dll"],
            ApiId::WindowsPrintf(_) | ApiId::UserObject(_) => return &["user32.dll"],
            ApiId::Audio(_) => return &["winmm.dll"],
            ApiId::FontResource(_) => return &["gdi32.dll"],
            ApiId::Path(id) => {
                return match id {
                    catalog::PathApi::UniversalNameA | catalog::PathApi::UniversalNameW => {
                        &["mpr.dll"]
                    }
                    catalog::PathApi::MakeSureDirectoryPathExists => &["imagehlp.dll"],
                    _ => &["shlwapi.dll"],
                }
            }
            ApiId::Shell(_) => return &["shell32.dll"],
            ApiId::Heap(catalog::HeapApi::QueryInformation) => {
                return if self.mode == Mode::X86 {
                    &["kernel32.dll"]
                } else {
                    &["kernel32.dll", "kernelbase.dll"]
                }
            }
            ApiId::Dialog(_)
            | ApiId::WindowControl(
                catalog::WindowControlApi::DefaultDialogA
                | catalog::WindowControlApi::DefaultDialogW
                | catalog::WindowControlApi::Enable
                | catalog::WindowControlApi::Parent
                | catalog::WindowControlApi::SendTimeoutA
                | catalog::WindowControlApi::SendTimeoutW,
            )
            | ApiId::WindowQuery(
                catalog::WindowQueryApi::Desktop
                | catalog::WindowQueryApi::Zoomed
                | catalog::WindowQueryApi::Enabled
                | catalog::WindowQueryApi::MapPoints,
            ) => return &["user32.dll"],
            ApiId::Bcrypt(catalog::BcryptApi::SystemRandom) => return &["advapi32.dll"],
            ApiId::Bcrypt(_) => return &["bcrypt.dll"],
            ApiId::Crypto(_) => return &["advapi32.dll"],
            ApiId::Etw(_) => return &["advapi32.dll"],
            ApiId::Cursor(_) | ApiId::Image(_) => return &["user32.dll"],
            ApiId::WinHttp(_) => return &["winhttp.dll"],
            ApiId::Winsock(_) => return &["ws2_32.dll"],
            id @ (ApiId::Bstr(_) | ApiId::Variant(_)) => {
                return super::automation::dlls(self.mode, id)
            }
            ApiId::SafeArray(_) => return &["oleaut32.dll"],
            ApiId::Winrt(id) => return super::winrt::dlls(id),
            ApiId::Ime(_) => return &["imm32.dll"],
            ApiId::RawInput(_) => return &["user32.dll"],
            ApiId::Sspi(_) => return &["secur32.dll"],
            ApiId::Device(id) if self.mode == Mode::X64 => return super::devices::dlls(id),
            ApiId::Multimedia(_) => return &["winmm.dll"],
            ApiId::Registry(id) => return id.dlls(),
            ApiId::Token(_) | ApiId::Sid(_) => return &["advapi32.dll"],
            ApiId::Com(id) => return com::dlls(id),
            ApiId::Guid(_) => return &["ole32.dll", "combase.dll"],
            ApiId::Exception(ExceptionApi::CaptureContext | ExceptionApi::PcToFileHeader) => {
                return &["kernel32.dll", "kernelbase.dll", "ntdll.dll"]
            }
            ApiId::Exception(ExceptionApi::UnhandledFilter) => {
                return &["kernel32.dll", "kernelbase.dll"]
            }
            ApiId::Exception(ExceptionApi::VirtualUnwind | ExceptionApi::Unwind)
                if self.mode == Mode::X64 =>
            {
                return &["kernel32.dll", "kernelbase.dll", "ntdll.dll"]
            }
            ApiId::ThreadControl(
                catalog::ThreadControlApi::Context
                | catalog::ThreadControlApi::SetContext
                | catalog::ThreadControlApi::SetDescription
                | catalog::ThreadControlApi::GetDescription
                | catalog::ThreadControlApi::QueueApc,
            ) => return &["kernel32.dll", "kernelbase.dll"],
            ApiId::ThreadControl(catalog::ThreadControlApi::FreeAndExit) => {
                return &["kernel32.dll", "kernelbase.dll"]
            }
            ApiId::Snapshot(_) => return &["kernel32.dll", "kernelbase.dll"],
            ApiId::Identity(id) if !id.user() => return &["kernel32.dll", "kernelbase.dll"],
            ApiId::Version(
                catalog::VersionApi::RtlGetVersion | catalog::VersionApi::RtlVerifyVersionInfo,
            ) => {
                return if self.mode == Mode::X86 {
                    &["ntdll.dll"]
                } else {
                    &["kernel32.dll", "kernelbase.dll", "ntdll.dll"]
                }
            }
            ApiId::Version(_)
            | ApiId::File(
                catalog::FileApi::GetFullPathNameA | catalog::FileApi::GetFullPathNameW,
            ) => return &["kernel32.dll", "kernelbase.dll"],
            _ => {}
        }
        if matches!(contract.id, ApiId::Environment(_) | ApiId::SystemPath(_))
            || nls::implemented(contract.id)
            || calendar_services::implemented(contract.id)
            || file_mutations::implemented(contract.id)
            || matches!(
                contract.id,
                ApiId::File(
                    catalog::FileApi::PeekNamedPipe
                        | catalog::FileApi::CancelIo
                        | catalog::FileApi::CancelIoEx
                        | catalog::FileApi::OverlappedResult
                        | catalog::FileApi::OverlappedResultEx
                        | catalog::FileApi::OpenFileMappingA
                        | catalog::FileApi::OpenFileMappingW
                        | catalog::FileApi::Lock
                        | catalog::FileApi::Unlock
                        | catalog::FileApi::CancelSynchronous
                )
            )
            || matches!(
                contract.id,
                ApiId::PointerHelper(_)
                    | ApiId::Interlocked(_)
                    | ApiId::MessageFormat(_)
                    | ApiId::ModuleQuery(_)
                    | ApiId::ProcessQuery(_)
                    | ApiId::ProcessOption(_)
                    | ApiId::ProcessMemory(_)
                    | ApiId::ProcessControl(
                        catalog::ProcessControlApi::Create
                            | catalog::ProcessControlApi::Exit
                            | catalog::ProcessControlApi::Terminate
                    )
                    | ApiId::Debug(_)
            )
        {
            return &["kernel32.dll", "kernelbase.dll"];
        }
        if matches!(
            contract.id,
            ApiId::AddressWait(_) | ApiId::Delay(DelayApi::Sleep | DelayApi::SleepEx)
        ) {
            return &[
                "kernel32.dll",
                "kernelbase.dll",
                "api-ms-win-core-synch-l1-2-0.dll",
            ];
        }
        if self.mode != Mode::X64 {
            return &[];
        }
        match contract.id {
            ApiId::Heap(_) if heaps::implemented(contract.id) => {
                &["kernel32.dll", "kernelbase.dll"]
            }
            ApiId::GdiOperation(_) if !gdi::font_service(contract.id) => &["gdi32.dll"],
            ApiId::Message(
                MessageApi::Wait
                | MessageApi::SetTimer
                | MessageApi::KillTimer
                | MessageApi::WaitObjects
                | MessageApi::WaitObjectsEx
                | MessageApi::QueueStatus,
            )
            | ApiId::Painting(_)
            | ApiId::WindowState(
                WindowStateApi::SetActive
                | WindowStateApi::Capture
                | WindowStateApi::SetCapture
                | WindowStateApi::ReleaseCapture
                | WindowStateApi::LastPopup,
            ) => &["user32.dll"],
            _ => &[],
        }
    }

    fn resolve(&self, dll: &str, name: &str) -> Result<&'static Contract, String> {
        let canonical = self.resolve_canonical(dll, name);
        if canonical.is_ok() || !super::provider_aliases::declares(dll, name, self.mode) {
            return canonical;
        }
        if let Some(contract) = catalog::by_name(name) {
            if self
                .extended_dlls(contract)
                .iter()
                .chain(contract.dlls(self.architecture()))
                .any(|owner| {
                    !super::provider_aliases::known(owner)
                        && self.resolve_canonical(owner, name).is_ok()
                })
            {
                return Ok(contract);
            }
        }
        canonical
    }

    fn resolve_canonical(&self, dll: &str, name: &str) -> Result<&'static Contract, String> {
        let name = if self.mode == Mode::X64 {
            catalog::normalize_symbol(dll, name)
        } else {
            catalog::safe_array_symbol(
                dll,
                catalog::bstr_symbol(dll, catalog::winsock_symbol(dll, name)),
            )
        };
        // Admission belongs to this runtime; the shared legacy policy stays intact.
        if let Some(contract) = catalog::by_name(name).filter(|c| {
            self.extended_dlls(c)
                .iter()
                .any(|owner| owner.eq_ignore_ascii_case(dll))
        }) {
            return Ok(contract);
        }
        match catalog::resolve(self.architecture(), dll, name) {
            Resolution::Bound(contract)
                if !matches!(
                    contract.id,
                    ApiId::Geometry(_)
                        | ApiId::Display(_)
                        | ApiId::Registry(_)
                        | ApiId::Token(_)
                        | ApiId::Sid(_)
                        | ApiId::Com(_)
                        | ApiId::Guid(_)
                        | ApiId::Shell(_)
                        | ApiId::Bcrypt(_)
                        | ApiId::Etw(_)
                        | ApiId::Cursor(_)
                        | ApiId::WinHttp(_)
                        | ApiId::Winsock(_)
                        | ApiId::Image(_)
                        | ApiId::BasicString(_)
                        | ApiId::WindowsPrintf(_)
                        | ApiId::Bstr(_)
                        | ApiId::Variant(_)
                        | ApiId::Winrt(_)
                        | ApiId::Ime(_)
                        | ApiId::RawInput(_)
                        | ApiId::Sspi(_)
                        | ApiId::Device(_)
                        | ApiId::Multimedia(_)
                        | ApiId::Desktop(_)
                        | ApiId::ModuleQuery(_)
                        | ApiId::Resource(_)
                        | ApiId::UserObject(_)
                        | ApiId::Audio(_)
                        | ApiId::FontResource(_)
                        | ApiId::Path(_)
                ) =>
            {
                Ok(contract)
            }
            _ => Err("unknown Windows export or incorrect provider".into()),
        }
    }

    pub fn bind(&mut self, dll: &str, name: &str) -> Result<u64, String> {
        if self.mode == Mode::X86 {
            if let (Some(dll), Some(export)) = (crt::provider(dll), crt::Export::lookup(name)) {
                if let Some(index) = self.bindings.iter().position(|b| matches!(b, Binding::Crt { export: e, dll: d } if *e == export && *d == dll)) { return Ok(self.address(index)); }
                let address = self.address(self.bindings.len());
                if address >= gateways::layout(self.mode).services.end {
                    return Err("Windows gateway capacity exhausted".into());
                }
                self.register_provider(&dll)?;
                self.bindings.push(Binding::Crt { export, dll });
                return Ok(address);
            }
        }
        if self.mode == Mode::X86
            && (dll.eq_ignore_ascii_case("d3d9.dll") || dll.eq_ignore_ascii_case("d3d9"))
        {
            let method = d3d9::Method::export(name).ok_or("unknown D3D9 export")?;
            return self.register_d3d9(method);
        }
        if self.mode == Mode::X86
            && (dll.eq_ignore_ascii_case("dsound.dll") || dll.eq_ignore_ascii_case("dsound"))
        {
            let method = dsound::Method::export(name).ok_or("unknown DirectSound export")?;
            return self.register_sound(method);
        }
        if self.mode == Mode::X86
            && (dll.eq_ignore_ascii_case("gdiplus.dll") || dll.eq_ignore_ascii_case("gdiplus"))
        {
            let export = gdiplus::Export::lookup(name).ok_or("unknown GDI+ export")?;
            if export.stack > 64 {
                return Err("invalid GDI+ export ABI".into());
            }
            if let Some(index) = self
                .bindings
                .iter()
                .position(|b| matches!(b, Binding::GdiPlus(e) if e.name == export.name))
            {
                return Ok(self.address(index));
            }
            let address = self.address(self.bindings.len());
            if address >= gateways::layout(self.mode).services.end {
                return Err("Windows gateway capacity exhausted".into());
            }
            self.register_provider("gdiplus.dll")?;
            self.bindings.push(Binding::GdiPlus(export));
            return Ok(address);
        }
        let architecture = self.architecture();
        let contract = self.resolve(dll, name)?;
        if (!matches!(
            contract.execution(architecture),
            ExecutionPolicy::Typed | ExecutionPolicy::LegacyUnimplemented
        ) && !self.extended(contract))
            || !Self::implemented(contract.id)
        {
            return Err("Windows service has not been migrated to this runtime".into());
        }
        if let Some(index) = self
            .bindings
            .iter()
            .position(|bound| bound.contract().is_some_and(|c| std::ptr::eq(c, contract)))
        {
            self.register_provider(dll)?;
            return Ok(self.address(index));
        }
        let address = self.address(self.bindings.len());
        if address >= gateways::layout(self.mode).services.end {
            return Err("Windows gateway capacity exhausted".into());
        }
        self.register_provider(dll)?;
        self.bindings.push(Binding::Service(contract));
        Ok(address)
    }

    pub(crate) fn bind_import_trap(&mut self, dll: &str, symbol: &str) -> Result<u64, String> {
        let dll = crate::pe::linker::module_name(dll)?;
        if symbol.is_empty() || symbol.len() > 4096 || symbol.contains('\0') {
            return Err("invalid diagnostic import symbol".into());
        }
        if let Some(index) = self.bindings.iter().position(|binding| {
            matches!(binding, Binding::ImportTrap { dll: owner, symbol: name }
                if owner == &dll && name == symbol)
        }) {
            return Ok(self.address(index));
        }
        let address = self.address(self.bindings.len());
        if address >= gateways::layout(self.mode).services.end {
            return Err("Windows gateway capacity exhausted".into());
        }
        // Bound diagnostic metadata independently of the larger gateway area.
        if self.import_trap_count() >= 4096 {
            return Err("diagnostic import capacity exhausted".into());
        }
        self.register_provider(&dll)?;
        self.bindings.push(Binding::ImportTrap {
            dll,
            symbol: symbol.into(),
        });
        Ok(address)
    }

    pub fn import_trap_count(&self) -> usize {
        self.bindings
            .iter()
            .filter(|binding| matches!(binding, Binding::ImportTrap { .. }))
            .count()
    }

    fn binding(&self, pc: u64) -> Option<&Binding> {
        let offset = pc.checked_sub(self.address(0))?;
        if offset % 16 != 0 {
            return None;
        }
        self.bindings.get(usize::try_from(offset / 16).ok()?)
    }

    fn contract(&self, pc: u64) -> Option<&'static Contract> {
        self.binding(pc)?.contract()
    }
    pub(crate) fn is_bound(&self, pc: u64) -> bool {
        self.binding(pc).is_some()
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn requires_graphics(&self, pc: u64) -> bool {
        matches!(self.binding(pc), Some(Binding::Graphics(_)))
            || matches!(
                self.contract(pc).map(|c| c.id),
                Some(ApiId::GraphicsExport(
                    catalog::GraphicsExportApi::Factory
                        | catalog::GraphicsExportApi::Factory1
                        | catalog::GraphicsExportApi::Factory2
                        | catalog::GraphicsExportApi::D3d11Device
                ))
            )
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn requires_audio(&self, pc: u64) -> bool {
        matches!(self.binding(pc), Some(Binding::Sound(method))
            if matches!(method.name, "DirectSoundCreate" | "DirectSoundCreate8"))
    }

    pub fn clock_source(
        &self,
        state: &State,
        memory: &Memory,
        image: Option<&crate::process::Loaded>,
    ) -> Option<ClockSource> {
        if let Some(Binding::Crt { export, .. }) = self.binding(state.ip) {
            return Some(if export.wall() {
                ClockSource::Wall
            } else {
                ClockSource::None
            });
        }
        if matches!(
            self.binding(state.ip),
            Some(
                Binding::Graphics(_)
                    | Binding::Wmi(_)
                    | Binding::GdiPlus(_)
                    | Binding::D3D9(_)
                    | Binding::Sound(_)
            )
        ) {
            return Some(ClockSource::None);
        }
        if image.is_some_and(|i| i.process.exit.is_some()) {
            return Some(ClockSource::None);
        }
        if matches!(self.binding(state.ip), Some(Binding::ImportTrap { .. })) {
            return Some(ClockSource::None);
        }
        match self.contract(state.ip)?.id {
            ApiId::Winsock(id) if id != catalog::WinsockApi::WaitEvents => Some(ClockSource::None),
            ApiId::Clock(ClockApi::PerformanceFrequency) => Some(ClockSource::None),
            ApiId::Clock(ClockApi::SystemFileTime) => Some(ClockSource::Wall),
            ApiId::Clock(_) => Some(ClockSource::Elapsed),
            id if file_mutations::implemented(id) => {
                Some(if file_mutations::uses_wall(id, state, memory) {
                    ClockSource::Wall
                } else {
                    ClockSource::None
                })
            }
            ApiId::Calendar(id) if calendar_services::implemented(ApiId::Calendar(id)) => Some(
                calendar_services::clock_source(id, state, memory, &self.time_zone),
            ),
            ApiId::Context(_)
            | ApiId::Snapshot(_)
            | ApiId::ThreadControl(
                catalog::ThreadControlApi::SetDescription
                | catalog::ThreadControlApi::GetDescription
                | catalog::ThreadControlApi::FreeAndExit,
            )
            | ApiId::ModuleQuery(_)
            | ApiId::ProcessQuery(_)
            | ApiId::ProcessOption(_)
            | ApiId::ProcessMemory(_)
            | ApiId::ProcessControl(
                catalog::ProcessControlApi::Create
                | catalog::ProcessControlApi::Exit
                | catalog::ProcessControlApi::Terminate,
            )
            | ApiId::Debug(_)
            | ApiId::Module(_)
            | ApiId::Exception(_)
            | ApiId::GraphicsExport(_)
            | ApiId::GraphicsProbe(_)
            | ApiId::Certificate(_)
            | ApiId::Icmp(_)
            | ApiId::Accessibility(_)
            | ApiId::Console(_)
            | ApiId::Security(_)
            | ApiId::VirtualMemory(_)
            | ApiId::SystemProfile(_)
            | ApiId::Startup(_)
            | ApiId::Shell(_)
            | ApiId::Bcrypt(_)
            | ApiId::Crypto(_)
            | ApiId::Etw(_)
            | ApiId::Cursor(_)
            | ApiId::Image(_)
            | ApiId::BasicString(_)
            | ApiId::WindowsPrintf(_)
            | ApiId::WinHttp(_)
            | ApiId::Bstr(_)
            | ApiId::Variant(_)
            | ApiId::SafeArray(_)
            | ApiId::Winrt(_)
            | ApiId::Ime(_)
            | ApiId::RawInput(_)
            | ApiId::Sspi(_)
            | ApiId::Device(_)
            | ApiId::Multimedia(_)
            | ApiId::Environment(_)
            | ApiId::Identity(_)
            | ApiId::Version(_)
            | ApiId::SystemPath(_)
            | ApiId::VersionResource(_)
            | ApiId::Resource(_)
            | ApiId::UserObject(_)
            | ApiId::Audio(_)
            | ApiId::FontResource(_)
            | ApiId::Path(_)
            | ApiId::Menu(_)
            | ApiId::Registry(_)
            | ApiId::Token(_)
            | ApiId::Sid(_)
            | ApiId::Com(_)
            | ApiId::Guid(_)
            | ApiId::Geometry(_)
            | ApiId::Display(_)
            | ApiId::Desktop(_)
            | ApiId::Local(_)
            | ApiId::PointerHelper(_)
            | ApiId::Interlocked(_) => Some(ClockSource::None),
            ApiId::MessageFormat(_) => Some(ClockSource::None),
            id if messages::implemented(id) => {
                Some(if messages::uses_clock(id, state, memory, image) {
                    ClockSource::Elapsed
                } else {
                    ClockSource::None
                })
            }
            id if nls::implemented(id) => Some(nls::clock_source(id, state, memory)),
            id if user::implemented(id) || gdi::implemented(id) => Some(ClockSource::None),
            id if objects::implemented(id) => Some(objects::clock_source(id, state, memory, image)),
            id if threads::implemented(id) => {
                Some(if threads::uses_clock(id, state, memory, image) {
                    ClockSource::Elapsed
                } else {
                    ClockSource::None
                })
            }
            id if files::uses_wall(id) => {
                let handle = if self.mode == Mode::X64 {
                    Some(state.regs[1])
                } else {
                    let mut bytes = [0; 4];
                    memory
                        .observe(self.mode.address(state.regs[4].wrapping_add(4)), &mut bytes)
                        .ok()
                        .map(|()| u64::from(u32::from_le_bytes(bytes)))
                };
                Some(
                    if image.is_some_and(|image| {
                        handle.is_some_and(|h| image.open_files.auto_write_time(h))
                    }) {
                        ClockSource::Wall
                    } else {
                        ClockSource::None
                    },
                )
            }
            id if directories::implemented(id)
                || files::implemented(id)
                || heaps::implemented(id) =>
            {
                Some(ClockSource::None)
            }
            _ => None,
        }
    }

    /// A successful API charges one work unit and no guest retirement.
    /// Delay gateways only resolve and transfer the original call.
    /// Failed argument/output access leaves the CPU at its original gateway.
    pub fn dispatch(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
        mut image: Option<&mut crate::process::Loaded>,
        files: &mut crate::pe::linker::Files,
        platform: ServiceSample<'_>,
    ) -> Result<(), ServiceError> {
        let ServiceSample {
            elapsed: sample,
            wall,
            cpu_times,
            random,
        } = platform;
        if gateways::delay_area(self.mode).contains(&state.ip) {
            if image.as_ref().is_some_and(|i| !i.loader_available()) {
                return Err(ServiceError::LoaderBlocked);
            }
            return image
                .ok_or(ServiceError::Unbound)?
                .begin_delay(state, memory);
        }
        if let Some(Binding::ImportTrap { dll, symbol }) = self.binding(state.ip) {
            return Err(ServiceError::ImportTrap {
                dll: dll.clone(),
                symbol: symbol.clone(),
            });
        }
        if let Some(Binding::Crt { export, dll }) = self.binding(state.ip) {
            let export = *export;
            let dll = dll.clone();
            let image = image.ok_or(ServiceError::Unavailable("CRT requires a loaded image"))?;
            image.retire_escaped32(state)?;
            let call = if let Some(pending) = image
                .pending()
                .filter(|p| p.pc == state.ip && export.name == "_lock")
            {
                pending.call.clone()
            } else if matches!(export.name, "exit" | "_exit" | "quick_exit") {
                Call::capture_nonreturn32(state, memory, export.slots)?
            } else {
                Call::capture_cdecl32(state, memory, export.slots)?
            };
            if export.callback() {
                image.crt_callback(export.name, &dll, call, state, memory)?;
                state.work = state.work.wrapping_add(1);
                return Ok(());
            }
            let startup = super::crt_startup::dispatch(
                export.name,
                &call.arguments,
                &dll,
                image,
                self,
                memory,
            );
            let startup = match startup {
                Err(ServiceError::Blocked) if export.name == "_lock" => {
                    image.set_pending(
                        call,
                        crate::process::threads::PendingKind::Critical,
                        state.ip,
                    );
                    return Err(ServiceError::Blocked);
                }
                other => other?,
            };
            if export.name == "_lock" {
                image.clear_pending();
            }

            let value = if let Some(value) = startup {
                value
            } else {
                crt::dispatch(
                    export,
                    &call.arguments,
                    state,
                    memory,
                    image,
                    export.wall().then_some(sample),
                )?
            };
            call.complete(state, value);
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let Some(Binding::Graphics(method)) = self.binding(state.ip) {
            let method = *method;
            let image = image.ok_or(ServiceError::Unavailable(
                "graphics requires a loaded image",
            ))?;
            let call = match image.graphics.saved_call(state.ip) {
                Some(call) => call.clone(),
                None => Call::capture_graphics(
                    state,
                    memory,
                    method.arity(),
                    method.dwords(),
                    method.floats(),
                )?,
            };
            image
                .graphics
                .dispatch(Err(method), call, state, memory, self, &image.user)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let Some(Binding::Wmi(method)) = self.binding(state.ip) {
            let method = *method;
            let call = Call::capture_stdcall32(state, memory, method.stack)?;
            let image = image.ok_or(ServiceError::Unavailable("WMI requires a loaded image"))?;
            image.retire_escaped32(state)?;
            let value = wmi::dispatch(Some(method), &call.arguments, memory, image, self)?;
            call.complete(state, value);
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let Some(Binding::GdiPlus(export)) = self.binding(state.ip) {
            let export = *export;
            let call = Call::capture_stdcall32(state, memory, export.stack)?;
            let image = image.ok_or(ServiceError::Unavailable("GDI+ requires a loaded image"))?;
            image.retire_escaped32(state)?;
            let value = gdiplus::dispatch(export, &call.arguments, memory, image)?;
            call.complete(state, value);
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let Some(Binding::D3D9(method)) = self.binding(state.ip) {
            let method = *method;
            let call = Call::capture_stdcall32(state, memory, method.stack)?;
            let image = image.ok_or(ServiceError::Unavailable("D3D9 requires a loaded image"))?;
            image.retire_escaped32(state)?;
            let value = d3d9::dispatch(method, &call.arguments, memory, image, self)?;
            call.complete(state, value);
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let Some(Binding::Sound(method)) = self.binding(state.ip) {
            let method = *method;
            let call = Call::capture_stdcall32(state, memory, method.stack)?;
            let image = image.ok_or(ServiceError::Unavailable(
                "DirectSound requires a loaded image",
            ))?;
            image.retire_escaped32(state)?;
            let value = dsound::dispatch(method, &call.arguments, memory, image, self)?;
            call.complete(state, value);
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        let contract = self.contract(state.ip).ok_or(ServiceError::Unbound)?;
        if self.mode == Mode::X86 {
            if let Some(image) = image.as_mut() {
                image.retire_escaped32(state)?;
            }
        }
        if matches!(
            contract.id,
            ApiId::Module(_) | ApiId::ThreadControl(catalog::ThreadControlApi::FreeAndExit)
        ) && image.as_ref().is_some_and(|i| !i.loader_available())
        {
            return Err(ServiceError::LoaderBlocked);
        }
        if threads::implemented(contract.id) {
            return threads::dispatch(contract, self, state, memory, image, sample);
        }
        if messages::implemented(contract.id) {
            return messages::dispatch(contract, self, state, memory, image, sample);
        }
        if contract.id == ApiId::ProcessControl(catalog::ProcessControlApi::Create) {
            let call = match image
                .as_deref()
                .and_then(|image| super::file_input::saved(image, state.ip))
            {
                Some(call) => call.clone(),
                None => Call::capture(self.mode, state, memory, contract)?,
            };
            return process_create::dispatch(
                &self.time_zone,
                call,
                state,
                memory,
                image.ok_or(ServiceError::Unavailable(
                    "process creation requires a loaded image",
                ))?,
                files,
            );
        }
        if matches!(
            contract.id,
            ApiId::ProcessControl(
                catalog::ProcessControlApi::Create
                    | catalog::ProcessControlApi::Exit
                    | catalog::ProcessControlApi::Terminate
            )
        ) {
            return process_control::dispatch(
                contract,
                state,
                memory,
                image.ok_or(ServiceError::Unavailable(
                    "process control requires a loaded image",
                ))?,
            );
        }
        let call = match image.as_ref().and_then(|i| {
            i.graphics
                .saved_call(state.ip)
                .or_else(|| super::file_input::saved(i, state.ip))
        }) {
            Some(call) => call.clone(),
            None => Call::capture(self.mode, state, memory, contract)?,
        };
        if let ApiId::GraphicsExport(id) = contract.id {
            let image = image.ok_or(ServiceError::Unavailable(
                "graphics requires a loaded image",
            ))?;
            image
                .graphics
                .dispatch(Ok(id), call, state, memory, self, &image.user)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if contract.id == ApiId::Display(catalog::DisplayApi::Monitors) {
            image
                .ok_or(ServiceError::Unavailable(
                    "monitor enumeration requires a loaded image",
                ))?
                .enumerate_monitors(call, state, memory)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Nls(
            id @ (catalog::NlsApi::EnumSystemLocalesA
            | catalog::NlsApi::EnumSystemLocalesW
            | catalog::NlsApi::EnumSystemLocalesEx),
        ) = contract.id
        {
            image
                .ok_or(ServiceError::Unavailable(
                    "locale enumeration requires a loaded image",
                ))?
                .enumerate_locales(id, call, state, memory)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Debug(id) = contract.id {
            let image = image.ok_or(ServiceError::Unavailable(
                "debug output requires a loaded image",
            ))?;
            if id == catalog::DebugApi::Break {
                image.debug_break(&call, state, memory)?;
            } else {
                image.output_debug_string(id == catalog::DebugApi::StringW, call, state, memory)?;
            }
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Local(id) = contract.id {
            locals::dispatch(
                id,
                call,
                state,
                memory,
                image.ok_or(ServiceError::Unavailable(
                    "local slots require a loaded image",
                ))?,
            )?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if user::implemented(contract.id) {
            let image = image.ok_or(ServiceError::Unavailable(
                "USER services require a loaded image",
            ))?;
            match contract.id {
                ApiId::Class(id) => {
                    let value =
                        super::class_services::dispatch(id, &call, state, memory, image, self)?;
                    call.complete(state, value);
                }
                ApiId::WindowQuery(id) => {
                    let value = if id == catalog::WindowQueryApi::Desktop {
                        super::class_services::desktop(image, memory, state, self)?
                    } else {
                        image.user.query(id, &call.arguments, state, memory, self)?
                    };
                    call.complete(state, value);
                }
                ApiId::Painting(id) => image.painting_call(id, call, state, memory, self)?,
                ApiId::WindowControl(id) => {
                    if matches!(
                        id,
                        catalog::WindowControlApi::CreateA | catalog::WindowControlApi::CreateW
                    ) && !super::class_services::prepare_window(
                        image,
                        &call,
                        memory,
                        state,
                        self,
                        id.wide(),
                    )? {
                        call.complete(state, 0);
                    } else {
                        image.window_call(id, call, state, memory, self)?;
                    }
                }
                ApiId::WindowState(id) => image.window_state_call(id, call, state, memory, self)?,
                ApiId::Dialog(id) => image.dialog_call(id, call, state, memory, self)?,
                ApiId::Keyboard(id) => {
                    let value = keyboard::dispatch(id, &call, state, memory, image)?;
                    call.complete(state, value);
                }
                _ => unreachable!(),
            }
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Exception(ExceptionApi::LookupEntry) = contract.id {
            image
                .ok_or(ServiceError::Unavailable(
                    "function lookup requires a loaded image",
                ))?
                .lookup_function(call, state, memory)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Exception(id @ (ExceptionApi::Unwind | ExceptionApi::UnhandledFilter)) =
            contract.id
        {
            let image = image.ok_or(ServiceError::Unavailable(
                "exception services require a loaded image",
            ))?;
            if id == ExceptionApi::Unwind && self.mode == Mode::X64 {
                image.unwind_exception(&call, false, state, memory)?;
            } else {
                image.exception_call(id, call, state, memory)?;
            }
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Exception(ExceptionApi::UnwindEx) = contract.id {
            image
                .ok_or(ServiceError::Unavailable(
                    "exception services require a loaded image",
                ))?
                .unwind_exception(&call, true, state, memory)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Exception(ExceptionApi::Raise) = contract.id {
            image
                .ok_or(ServiceError::Unavailable(
                    "exception services require a loaded image",
                ))?
                .raise_exception(call, state, memory)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if let ApiId::Module(
            id @ (ModuleApi::LoadLibraryA
            | ModuleApi::LoadLibraryW
            | ModuleApi::LoadLibraryExA
            | ModuleApi::LoadLibraryExW
            | ModuleApi::FreeLibrary
            | ModuleApi::GetProcAddress),
        ) = contract.id
        {
            modules::control(self, id, call, state, memory, image, files)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if contract.id == ApiId::ThreadControl(catalog::ThreadControlApi::FreeAndExit) {
            let image = image.ok_or(ServiceError::Unavailable(
                "thread exit requires a loaded image",
            ))?;
            match image.free_and_exit_thread(
                call.arguments[0],
                call.arguments[1] as u32,
                state,
                memory,
                self,
            ) {
                Ok(()) => {}
                Err(crate::process::LoadError::Fault(fault)) => {
                    return Err(ServiceError::Fault(fault))
                }
                Err(crate::process::LoadError::Windows(code)) => {
                    debug_assert_eq!(code, 170);
                    return Err(ServiceError::Unavailable(
                        "FreeLibraryAndExitThread cannot leave an active loader callback",
                    ));
                }
            }
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        let result = match contract.id {
            ApiId::Snapshot(id) => snapshots::dispatch(
                id,
                &call,
                state,
                memory,
                image.ok_or(ServiceError::Unavailable(
                    "snapshots require a loaded image",
                ))?,
            )?,
            ApiId::ThreadControl(
                id @ (catalog::ThreadControlApi::SetDescription
                | catalog::ThreadControlApi::GetDescription),
            ) => thread_descriptions::dispatch(
                id,
                &call,
                memory,
                image.ok_or(ServiceError::Unavailable(
                    "thread descriptions require a loaded image",
                ))?,
            )?,
            ApiId::Exception(ExceptionApi::VirtualUnwind) => {
                crate::process::virtual_unwind_call(&call, memory)?
            }
            ApiId::Exception(ExceptionApi::PcToFileHeader) => image
                .ok_or(ServiceError::Unavailable(
                    "module lookup requires a loaded image",
                ))?
                .pc_to_file_header(&call, memory)?,
            ApiId::Exception(ExceptionApi::CaptureContext) => {
                crate::process::capture_call(&call, state, memory)?
            }
            ApiId::File(id) if file_mutations::implemented(contract.id) => {
                file_mutations::dispatch(id, &call, state, memory, files, image, sample)?
            }
            id if heaps::implemented(id) => heaps::dispatch(contract, &call, state, memory, image)?,
            ApiId::Startup(id) => startup::dispatch(id, &call, memory, image.as_deref())?,
            ApiId::Shell(id) => shell::dispatch(id, &call, state, memory, image, files)?,
            ApiId::Etw(id) => etw::dispatch(id, &call, memory, image)?,
            ApiId::Winsock(id) => super::winsock::dispatch(id, &call, state, memory, image)?,
            ApiId::WinHttp(id) => super::winhttp::dispatch(id, &call, state, memory)?,
            ApiId::Device(id) => super::devices::dispatch(id, &call, state, memory, image)?,
            ApiId::Multimedia(id) => super::multimedia::dispatch(id, &call, memory, image)?,
            ApiId::Sspi(id) => super::sspi::dispatch(id, &call, memory, image, self)?,
            id @ (ApiId::Bstr(_) | ApiId::Variant(_)) => {
                super::automation::dispatch(id, &call, state, memory, image)?
            }
            ApiId::SafeArray(id) => super::safe_arrays::dispatch(id, &call, memory, image)?,
            ApiId::Winrt(id) => super::winrt::dispatch(id, &call, memory, image)?,
            ApiId::Ime(id) => super::ime::dispatch(id, &call, state, memory, image.as_deref())?,
            ApiId::RawInput(id) => super::raw_input::dispatch(id, &call, state, memory, image)?,
            ApiId::Cursor(id) => cursor::dispatch(id, &call, state, memory, image)?,
            ApiId::Image(id) => super::images::dispatch(id, &call, state, memory, image)?,
            ApiId::BasicString(id) => super::basic_strings::dispatch(id, &call, state, memory)?,
            ApiId::Resource(id) => super::resources::dispatch(id, &call, state, memory, image)?,
            ApiId::Path(id) => super::paths::dispatch(id, &call, state, memory, files)?,
            ApiId::Menu(id) => super::menus::dispatch(id, &call, state, memory, image)?,
            ApiId::UserObject(id) => {
                super::user_objects::dispatch(id, &call, state, memory, image)?
            }
            ApiId::WindowsPrintf(id) => super::windows_printf::dispatch(id, &call, state, memory)?,
            id @ (ApiId::Audio(_) | ApiId::FontResource(_)) => {
                super::unavailable_endpoints::dispatch(id, &call, state, memory)?
            }
            ApiId::Bcrypt(id) => bcrypt::dispatch(id, &call, memory, image, random)?,
            ApiId::Crypto(id) => super::crypto::dispatch(id, &call, state, memory, image)?,
            ApiId::SystemProfile(id) => system_profile::dispatch(id, &call, state, memory, files)?,
            ApiId::Nls(id) => nls::dispatch(id, &call, state, memory, sample, &self.time_zone)?,
            ApiId::Identity(id) => identity::dispatch(id, &call, state, memory)?,
            ApiId::Version(id) => version::dispatch(id, &call, state, memory)?,
            ApiId::VersionResource(id) => {
                version_resources::dispatch(id, &call, state, memory, files, image.as_deref_mut())?
            }
            ApiId::Geometry(id) => geometry::dispatch(id, &call, state, memory)?,
            ApiId::Registry(id) => registry::dispatch(id, &call, memory, image)?,
            ApiId::Token(id) => tokens::dispatch(id, &call, state, memory, image)?,
            ApiId::Sid(id) => sids::dispatch(id, &call, state, memory)?,
            ApiId::Com(id) => com::dispatch(id, &call, memory, image, self)?,
            ApiId::Security(id) => super::security::dispatch(id, &call, state, memory, image)?,
            ApiId::Guid(id) => guid::dispatch(id, &call, memory, image, random)?,
            ApiId::Display(id) => display::dispatch(id, &call, state, memory, image.as_deref())?,
            ApiId::Desktop(id) => desktop::dispatch(id, &call, state, memory, image)?,
            ApiId::GraphicsProbe(id) => {
                graphics_probe::dispatch(id, &call, state, memory, image.as_deref())?
            }
            id @ (ApiId::Certificate(_)
            | ApiId::Icmp(_)
            | ApiId::Accessibility(_)
            | ApiId::Console(_)) => {
                super::platform_endpoints::dispatch(id, &call, state, memory, image.as_deref())?
            }
            ApiId::Calendar(id) => calendar_services::dispatch(
                id,
                &call,
                state,
                memory,
                sample,
                cpu_times,
                &self.time_zone,
            )?,
            ApiId::ModuleQuery(id) => {
                module_queries::dispatch(id, &call, state, memory, image.as_deref(), self)?
            }
            ApiId::ProcessQuery(id) => process_queries::dispatch(id, &call, state, memory, image)?,
            ApiId::ProcessOption(id) => process_options::dispatch(id, &call, state, memory, image)?,
            ApiId::ProcessMemory(id) => {
                process_memory::dispatch(id, &call, state, memory, image.as_deref())?
            }
            ApiId::Environment(id) => environment::dispatch(
                id,
                &call,
                state,
                memory,
                &mut image
                    .ok_or(ServiceError::Unavailable(
                        "environment services require a loaded image",
                    ))?
                    .environment,
            )?,
            ApiId::SystemPath(id) => system_paths::dispatch(
                id,
                &call,
                state,
                memory,
                &image
                    .ok_or(ServiceError::Unavailable(
                        "system paths require a loaded image",
                    ))?
                    .environment,
                files,
            )?,
            ApiId::VirtualMemory(id) => {
                virtual_memory::dispatch(id, &call, state, memory, image.as_deref())?
            }
            ApiId::PointerHelper(PointerHelperApi::FlushCache) => {
                virtual_memory::flush(&call, state, memory, image.as_deref())?
            }
            ApiId::PointerHelper(id) => pointer_helpers::dispatch(id, &call, memory),
            ApiId::Interlocked(id) => interlocked::dispatch(id, &call, memory)?,
            ApiId::MessageFormat(id) => message_format::dispatch(
                id,
                &call,
                state,
                memory,
                image.map(|image| &mut image.heaps),
            )?,
            ApiId::GdiDc(id) => gdi::dc(
                id,
                &call,
                state,
                memory,
                image.ok_or(ServiceError::Unavailable(
                    "GDI services require a loaded image",
                ))?,
            )?,
            ApiId::GdiOperation(id) => gdi::operation(
                id,
                &call,
                state,
                memory,
                image.ok_or(ServiceError::Unavailable(
                    "GDI services require a loaded image",
                ))?,
            )?,
            ApiId::Exception(id) => image
                .ok_or(ServiceError::Unavailable(
                    "exception services require a loaded image",
                ))?
                .exception_registration(id, &call, state, memory)?,
            id if objects::implemented(id) => {
                objects::dispatch(id, &call, state, memory, image, sample, wall)?
            }
            id if files::implemented(id) => {
                files::dispatch(id, &call, state, memory, files, image, sample)?
            }
            id if directories::implemented(id) => {
                directories::dispatch(id, &call, state, memory, image, files)?
            }
            ApiId::Context(id) => context::dispatch(id, call.arguments[0], state, memory)?,
            ApiId::Module(id) => modules::dispatch(self, id, &call, state, memory, image, files)?,
            ApiId::Clock(id) => match id {
                ClockApi::TickCount | ClockApi::MultimediaTick => (sample / 10_000) as u32 as u64,
                ClockApi::TickCount64 => sample / 10_000,
                ClockApi::PerformanceCounter | ClockApi::PerformanceFrequency => {
                    let value = if id == ClockApi::PerformanceFrequency {
                        u64::from(crate::cpu::CLOCK_FREQUENCY)
                    } else {
                        sample
                    };
                    memory.write_int(call.arguments[0], 8, value)?;
                    1
                }
                ClockApi::SystemFileTime => {
                    memory.write_int(call.arguments[0], 8, sample)?;
                    0
                }
            },
            _ => return Err(ServiceError::Unbound),
        };
        call.complete(state, result);
        state.work = state.work.wrapping_add(1);
        Ok(())
    }
}
