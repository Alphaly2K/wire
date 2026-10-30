//! Static dependency discovery precedes binding, allowing ordinary circular
//! imports to resolve against published image identities and export tables.
use super::search::{self, Search};
use super::*;
pub use crate::filesystem::Files;
use crate::win32::{catalog, providers, Services};
use std::collections::{BTreeMap, BTreeSet};

pub fn module_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.len() > 260
        || name.contains(['\\', '/', ':', '\0'])
        || name == "."
        || name == ".."
    {
        return Err("invalid supplied module identity".into());
    }
    let mut name = name.to_ascii_lowercase();
    if !name.contains('.') {
        name.push_str(".dll");
    }
    Ok(name)
}

/// Whether a normalized DLL name is resolved by the built-in provider layer.
pub fn provider(name: &str, mode: Mode) -> bool {
    let architecture = if mode == Mode::X86 {
        catalog::Architecture::X86
    } else {
        catalog::Architecture::X64
    };
    providers::native_module(name)
        || name == "kernelbase.dll"
        || name == "api-ms-win-core-synch-l1-2-0.dll"
        || catalog::contracts().any(|c| c.dlls(architecture).contains(&name))
        || Services::extends_provider(name, mode)
}

fn diagnostic_provider(name: &str, mode: Mode) -> bool {
    provider(name, mode)
        || matches!(
            name,
            "opengl32.dll"
                | "bcrypt.dll"
                | "crypt32.dll"
                | "d3d11.dll"
                | "d3d12.dll"
                | "dxgi.dll"
                | "dwmapi.dll"
                | "hid.dll"
                | "setupapi.dll"
                | "iphlpapi.dll"
                | "uiautomationcore.dll"
                | "winhttp.dll"
                | "combase.dll"
                | "api-ms-win-core-winrt-l1-1-0.dll"
                | "api-ms-win-core-winrt-string-l1-1-0.dll"
        )
}

#[derive(Debug)]
pub struct Error {
    pub code: u32,
    pub message: String,
}
impl Error {
    fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::new(193, message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::new(193, message)
    }
}
impl From<crate::memory::Fault> for Error {
    fn from(fault: crate::memory::Fault) -> Self {
        Self::from(fault.to_string())
    }
}

impl Files {
    pub fn supply(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let name = module_name(name)?;
        self.insert(&format!("C:\\{name}"), bytes.to_vec())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Mapped,
    Initializing,
    Ready,
    Detaching,
    Failed,
}

pub struct Module {
    pub name: String,
    pub path: String,
    pub base: u64,
    pub image: Image,
    pub exports: Exports,
    pub dependencies: Vec<u64>,
    pub references: u32,
    pub pinned: bool,
    pub thread_calls_disabled: bool,
    pub phase: Phase,
    pub tls: Option<ModuleTls>,
    pub delays: Vec<Delay>,
}
pub struct ModuleTls {
    pub slot: usize,
    pub blocks: BTreeMap<u32, u64>,
    pub template: Vec<u8>,
    pub size: u64,
    pub callbacks: Vec<u64>,
}

struct Pending {
    module: Module,
    bytes: Vec<u8>,
    imports: Vec<ImportDll>,
}

struct Linker<'a> {
    existing: &'a Memory,
    startup: &'a mut Memory,
    files: &'a Files,
    loaded: Option<&'a Linked>,
    edges: Vec<(u64, u64)>,
    pending: Vec<Pending>,
    names: BTreeMap<String, u64>,
    staged_bytes: u64,
    delay_remaining: usize,
    search: &'a Search,
    flags: u32,
    diagnostic_imports: bool,
    altered: Option<String>,
    open_files: Option<&'a crate::file_handles::Handles>,
}

pub struct Linked {
    pub modules: BTreeMap<u64, Module>,
    pub order: Vec<u64>,
    pub root: u64,
}
impl Linked {
    pub fn base(&self, name: &str) -> u64 {
        self.base_at(name, "C:\\")
    }
    pub(crate) fn base_at(&self, name: &str, cwd: &str) -> u64 {
        let Ok(name) = search::filename(name) else {
            return 0;
        };
        if search::qualified(&name) {
            let Ok(path) = crate::paths::logical_path_at(&name, cwd) else {
                return 0;
            };
            return self
                .modules
                .values()
                .find(|m| m.path.eq_ignore_ascii_case(&path))
                .map_or(0, |m| m.base);
        }
        self.modules
            .values()
            .find(|m| m.name.eq_ignore_ascii_case(&name))
            .map_or(0, |m| m.base)
    }
}

fn symbol(function: &ImportFunc) -> Result<Symbol, String> {
    if let Some(name) = &function.name {
        Ok(Symbol::Name(name.clone()))
    } else {
        function
            .ordinal
            .map(Symbol::Ordinal)
            .ok_or_else(|| "PE import has no identity".into())
    }
}

impl Linker<'_> {
    fn provider(&self, name: &str) -> bool {
        if self.diagnostic_imports {
            diagnostic_provider(name, self.existing.mode())
        } else {
            provider(name, self.existing.mode())
        }
    }

    fn module(&self, base: u64) -> &Module {
        self.pending
            .iter()
            .find(|p| p.module.base == base)
            .map(|p| &p.module)
            .or_else(|| self.loaded.and_then(|l| l.modules.get(&base)))
            .unwrap()
    }
    fn edge(&mut self, from: u64, to: u64) {
        if let Some(module) = self
            .pending
            .iter_mut()
            .find(|p| p.module.base == from)
            .map(|p| &mut p.module)
        {
            if !module.dependencies.contains(&to) {
                module.dependencies.push(to);
            }
        } else if !self.module(from).dependencies.contains(&to) && !self.edges.contains(&(from, to))
        {
            self.edges.push((from, to));
        }
    }
    fn available(&self, base: u64, size: u64) -> bool {
        self.existing.available(base, size)
            && self.startup.available(base, size)
            && self.pending.iter().all(|p| {
                base + size <= p.module.base || base >= p.module.base + p.module.image.mapped_size()
            })
    }
    fn discover(
        &mut self,
        path: String,
        bytes: &[u8],
        requested: u64,
        main: bool,
    ) -> Result<u64, Error> {
        if let Some(&index) = self.names.get(&path.to_lowercase()) {
            return Ok(index);
        }
        if let Some(open) = self.open_files {
            open.check_share(&path, 1, 5)
                .map_err(|code| Error::new(code, "PE image file is not shared for loading"))?;
        }
        if self.pending.len() + self.loaded.map_or(0, |l| l.modules.len()) >= 64 {
            return Err(Error::new(8, "loaded PE module capacity exceeded"));
        }
        let mut image = Image::parse(bytes)?;
        if image.mode != self.existing.mode() {
            return Err("dependency PE machine does not match the VM".into());
        }
        image.dep_enabled = self.startup.dep_enabled();
        if !main && !image.is_dll {
            return Err("PE dependency is not a DLL".into());
        }
        if image.entry_rva != 0 && !image.executable(image.entry_rva) {
            return Err("PE DLL entry is not executable".into());
        }
        let size = image.mapped_size();
        if self.staged_bytes + size > 512 * 1024 * 1024 {
            return Err(Error::new(
                8,
                "PE dependency graph staging capacity exceeded",
            ));
        }
        let base = if requested != 0 {
            requested
        } else if self.available(image.preferred_base, size) {
            image.preferred_base
        } else {
            let mut next = if image.mode == Mode::X86 {
                0x1000_0000u64
            } else {
                0x0001_8000_0000
            };
            let stride = size.div_ceil(65536) * 65536;
            while !self.available(next, size) {
                next = next
                    .checked_add(stride)
                    .filter(|&base| {
                        base + size
                            <= if image.mode == Mode::X86 {
                                0x7fff0000
                            } else {
                                1 << 47
                            }
                    })
                    .ok_or("PE image address space exhausted")?;
            }
            next
        };
        if !self.available(base, size) {
            return Err("PE load base overlaps an existing mapping or startup region".into());
        }
        let relocated = image.relocated(bytes, base)?;
        self.staged_bytes += size;
        let imports = image.imports(&relocated[..image.size as usize])?;
        let delays = image.delay_imports(&relocated, base, self.delay_remaining)?;
        self.delay_remaining -= delays.iter().map(|d| d.functions.len()).sum::<usize>();
        let exports = image.exports(&relocated)?;
        let index = self.pending.len();
        let name = module_name(path.rsplit('\\').next().unwrap())?;
        self.names.insert(path.to_lowercase(), base);
        self.pending.push(Pending {
            module: Module {
                path,
                name,
                base,
                image,
                exports,
                dependencies: Vec::new(),
                references: u32::from(main),
                pinned: main,
                thread_calls_disabled: false,
                phase: Phase::Mapped,
                tls: None,
                delays,
            },
            bytes: relocated,
            imports,
        });
        let dependencies: Vec<_> = self.pending[index]
            .imports
            .iter()
            .map(|i| {
                search::filename(&i.dll).map_err(|_| "invalid PE import module name".to_owned())
            })
            .collect::<Result<_, _>>()?;
        for name in dependencies {
            if self
                .search
                .provider(&name, self.files)
                .map_err(|_| "invalid PE import path")?
                .is_none_or(|name| !self.provider(&name))
            {
                let target = self.ensure(&name)?;
                self.edge(base, target);
            }
        }
        Ok(base)
    }
    fn ensure(&mut self, name: &str) -> Result<u64, Error> {
        if search::qualified(name)
            && self.provider(&name.rsplit('\\').next().unwrap().to_ascii_lowercase())
            && self
                .search
                .provider(name, self.files)
                .map_err(|c| Error::new(c, "invalid PE provider path"))?
                .is_none()
        {
            return Err(Error::new(
                126,
                "PE provider requires its canonical system path",
            ));
        }
        let path = if search::qualified(name) {
            Some(
                self.files
                    .resolve(name)
                    .map_err(|c| Error::new(c, "invalid PE dependency path"))?,
            )
        } else {
            None
        };
        let existing = self
            .pending
            .iter()
            .map(|p| &p.module)
            .chain(self.loaded.into_iter().flat_map(|l| l.modules.values()))
            .find(|m| {
                path.as_ref().map_or_else(
                    || m.name.eq_ignore_ascii_case(name),
                    |path| m.path.eq_ignore_ascii_case(path),
                )
            })
            .map(|m| m.base);
        if let Some(base) = existing {
            if matches!(self.module(base).phase, Phase::Detaching | Phase::Failed) {
                return Err(Error::new(
                    1114,
                    "PE dependency is not available for initialization",
                ));
            }
            return Ok(base);
        }
        let paths = self
            .search
            .candidates(name, self.flags, self.files, self.altered.as_deref())
            .map_err(|c| Error::new(c, "invalid PE dependency search"))?;
        let (path, bytes) = self.files.find(paths).map_err(|c| {
            Error::new(
                if matches!(c, 2 | 3) { 126 } else { c },
                format!("PE dependency bytes were not supplied: {name}"),
            )
        })?;
        self.discover(path, &bytes, 0, false)
    }
    fn resolve(
        &mut self,
        base: u64,
        importer: u64,
        symbol: Symbol,
        services: &mut Services,
        chain: &mut BTreeSet<(u64, Symbol)>,
    ) -> Result<u64, Error> {
        if chain.len() >= 32 || !chain.insert((base, symbol.clone())) {
            return Err(Error::new(127, "cyclic or overlong PE export forwarder"));
        }
        let target = self
            .module(base)
            .exports
            .get(&symbol)
            .cloned()
            .ok_or_else(|| Error::new(127, "PE imported export is missing"))?;
        let value = match target {
            Export::Address(rva) => base + u64::from(rva),
            Export::Forward { dll, symbol } => {
                if self.provider(&dll) {
                    self.bind_provider(&dll, &symbol, services, self.diagnostic_imports)?
                } else {
                    let next = self.ensure(&dll)?;
                    self.edge(base, next);
                    self.edge(importer, next);
                    self.resolve(next, importer, symbol, services, chain)?
                }
            }
        };
        Ok(value)
    }
    fn bind_provider(
        &mut self,
        dll: &str,
        symbol: &Symbol,
        services: &mut Services,
        diagnostic_imports: bool,
    ) -> Result<u64, Error> {
        let name = symbol.provider_name();
        let data = self.existing.mode() == Mode::X86
            && crate::win32::crt::provider(dll).is_some()
            && crate::win32::crt_startup::data_export(&name);
        let blocked: Vec<_> = self
            .pending
            .iter()
            .map(|p| (p.module.base, p.module.image.mapped_size()))
            .collect();
        match services.bind_for_loader(dll, &name, self.startup, self.existing, &blocked) {
            Ok(address) => Ok(address),
            Err(message) if data => Err(Error::new(8, message)),
            Err(_) if diagnostic_imports => services
                .bind_import_trap(dll, &name)
                .map_err(|message| Error::new(127, message)),
            Err(message) => Err(Error::new(127, message)),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn link(
    existing: &Memory,
    memory: &mut Memory,
    files: &Files,
    services: &mut Services,
    bytes: &[u8],
    requested: u64,
    search: &Search,
    diagnostic_imports: bool,
) -> Result<Linked, Error> {
    graph(
        existing,
        memory,
        files,
        services,
        None,
        &search.executable,
        bytes,
        requested,
        true,
        4096,
        search,
        0,
        None,
        diagnostic_imports,
    )
    .map(|(linked, _)| linked)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn extend(
    existing: &Memory,
    memory: &mut Memory,
    files: &Files,
    services: &mut Services,
    loaded: &Linked,
    name: &str,
    delay_remaining: usize,
    search: &Search,
    flags: u32,
    open_files: &crate::file_handles::Handles,
    diagnostic_imports: bool,
) -> Result<(Linked, Vec<(u64, u64)>), Error> {
    let paths = search
        .candidates(name, flags, files, None)
        .map_err(|c| Error::new(c, "invalid PE module search"))?;
    let (path, bytes) = files
        .find(paths)
        .map_err(|c| Error::new(c, format!("PE dependency bytes were not supplied: {name}")))?;
    graph(
        existing,
        memory,
        files,
        services,
        Some(loaded),
        &path,
        &bytes,
        0,
        false,
        delay_remaining,
        search,
        flags,
        Some(open_files),
        diagnostic_imports,
    )
}

#[allow(clippy::too_many_arguments)]
fn graph(
    existing: &Memory,
    memory: &mut Memory,
    files: &Files,
    services: &mut Services,
    loaded: Option<&Linked>,
    name: &str,
    bytes: &[u8],
    requested: u64,
    main: bool,
    delay_remaining: usize,
    search: &Search,
    flags: u32,
    open_files: Option<&crate::file_handles::Handles>,
    diagnostic_imports: bool,
) -> Result<(Linked, Vec<(u64, u64)>), Error> {
    let mut linker = Linker {
        existing,
        startup: memory,
        files,
        loaded,
        edges: Vec::new(),
        pending: Vec::new(),
        names: loaded
            .map(|l| {
                l.modules
                    .values()
                    .map(|m| (m.path.to_lowercase(), m.base))
                    .collect()
            })
            .unwrap_or_default(),
        delay_remaining,
        search,
        flags,
        diagnostic_imports,
        altered: (flags & (8 | 0x100) != 0).then(|| search::parent(name).to_owned()),
        open_files,
        staged_bytes: loaded.map_or(0, |l| {
            l.modules.values().map(|m| m.image.mapped_size()).sum()
        }),
    };
    let root = linker.discover(name.to_owned(), bytes, requested, main)?;
    let mut index = 0;
    while index < linker.pending.len() {
        let from = linker.pending[index].module.base;
        for dll in linker.pending[index].imports.clone() {
            let name = search::filename(&dll.dll)
                .map_err(|c| Error::new(c, "invalid PE import module name"))?;
            let builtin = search
                .provider(&name, files)
                .map_err(|c| Error::new(c, "invalid PE import module path"))?
                .filter(|name| linker.provider(name));
            let is_provider = builtin.is_some();
            if is_provider {
                services.register_provider(builtin.as_deref().unwrap())?;
            }
            for function in dll.functions {
                let symbol = symbol(&function)?;
                let value = if is_provider {
                    linker.bind_provider(
                        builtin.as_deref().unwrap(),
                        &symbol,
                        services,
                        diagnostic_imports,
                    )?
                } else {
                    let next = linker.ensure(&name)?;
                    if linker.module(next).phase == Phase::Detaching {
                        return Err(Error::new(1114, "PE dependency is detaching"));
                    }
                    linker.edge(from, next);
                    linker.resolve(next, from, symbol, services, &mut BTreeSet::new())?
                };
                let pending = &mut linker.pending[index];
                pending
                    .module
                    .image
                    .patch_import(&mut pending.bytes, function.iat_rva, value)?;
            }
        }
        index += 1;
    }
    fn visit(base: u64, linker: &Linker<'_>, seen: &mut BTreeSet<u64>, order: &mut Vec<u64>) {
        if !seen.insert(base) {
            return;
        }
        for &dependency in &linker.module(base).dependencies {
            visit(dependency, linker, seen, order);
        }
        for &(from, to) in &linker.edges {
            if from == base {
                visit(to, linker, seen, order);
            }
        }
        if linker.pending.iter().any(|p| p.module.base == base) {
            order.push(base);
        }
    }
    let mut order = Vec::new();
    visit(root, &linker, &mut BTreeSet::new(), &mut order);
    let pending = linker.pending;
    let edges = linker.edges;
    let mut modules = BTreeMap::new();
    for pending in pending {
        memory.merge(
            pending
                .module
                .image
                .map_bytes(pending.module.base, &pending.bytes)?,
        )?;
        modules.insert(pending.module.base, pending.module);
    }
    Ok((
        Linked {
            modules,
            order,
            root,
        },
        edges,
    ))
}
