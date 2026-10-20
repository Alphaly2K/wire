//! PE32 graph linker borrows only module, ABI, filesystem and class resources.
//! It maps guest data and binds gateways; Process executes initialization.
use super::{
    bad, basename, library_name, memory, native_module, prepare, relocate, static_tls, Export,
    Failure, LoadResult, Module, Modules, Phase, StaticTls, Symbol, Transaction, TLS_SLOTS,
};
use crate::{
    api::ApiTable, fs::GuestFs, mem::GuestMem, user32::User32, win32::x86::scratch::Scratch32,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub(crate) struct Linker<'a> {
    modules: &'a mut Modules,
    api: &'a mut ApiTable,
    user32: &'a mut User32,
    scratch: &'a mut Scratch32,
    last_error: &'a mut u32,
    fs: Option<&'a GuestFs>,
    exe_path: &'a str,
    cmdline: [u32; 2],
}
impl<'a> Linker<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        modules: &'a mut Modules,
        api: &'a mut ApiTable,
        user32: &'a mut User32,
        scratch: &'a mut Scratch32,
        last_error: &'a mut u32,
        fs: Option<&'a GuestFs>,
        exe_path: &'a str,
        cmdline: [u32; 2],
    ) -> Self {
        Self {
            modules,
            api,
            user32,
            scratch,
            last_error,
            fs,
            exe_path,
            cmdline,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn link(
        &mut self,
        name: &str,
        parent: Option<&Path>,
        mem: &mut GuestMem,
        txn: &mut Transaction,
        depth: usize,
    ) -> LoadResult<u32> {
        if depth > 64 {
            return Err(bad("DLL dependency depth exceeded"));
        }
        let name = library_name(name);
        let short = basename(&name);
        if native_module(&short) {
            let handle = self.modules.provider_handle_for(&short);
            let classes: &[(&str, bool)] = match short.as_str() {
                "riched32.dll" => &[("RichEdit", false)],
                "riched20.dll" => &[("RichEdit20A", false), ("RichEdit20W", true)],
                "msftedit.dll" => &[("RICHEDIT50W", true)],
                _ => &[],
            };
            for &(name, wide) in classes {
                self.user32
                    .register_system_class(
                        mem,
                        self.api,
                        self.scratch,
                        self.last_error,
                        handle,
                        name,
                        wide,
                    )
                    .map_err(Failure::Fatal)?;
            }
            return Ok(handle);
        }
        // Consult the loaded module list before searching the filesystem.
        if !name.contains(['\\', '/', ':']) {
            if let Some(handle) = self.modules.handle(&name, self.fs) {
                self.modules.acquire(txn, handle)?;
                return Ok(handle);
            }
        }
        let fs = self
            .fs
            .ok_or_else(|| Failure::Expected(126, "guest filesystem unavailable".into()))?;
        let mut candidates = Vec::new();
        if !name.contains(['\\', '/', ':']) {
            if let Some(parent) = parent {
                candidates.push(parent.join(&name).to_string_lossy().into_owned());
            }
            let exe_dir = self.exe_path.rsplit_once('\\').map_or("C:\\", |(p, _)| p);
            candidates.push(format!("{exe_dir}\\{name}"));
        }
        candidates.push(name);
        let mut found = None;
        for candidate in candidates {
            let path = fs
                .resolve(&candidate)
                .map_err(|e| Failure::Expected(e, "invalid DLL path".into()))?;
            if path.is_file() {
                found = Some(path.canonicalize().map_err(memory)?);
                break;
            }
        }
        let path = found.ok_or_else(|| Failure::Expected(126, format!("DLL {short} not found")))?;
        if let Some(&handle) = self.modules.paths.get(&path) {
            self.modules.acquire(txn, handle)?;
            return Ok(handle);
        }
        let bytes = std::fs::read(&path).map_err(memory)?;
        let (pe, mut image, table) = prepare(&bytes)?;
        let imports = pe.imports(&bytes).map_err(bad)?;
        let (base, size) = self.modules.allocate(&pe, mem)?;
        relocate(&pe, &mut image, base)?;
        let tls = if let Some(spec) = static_tls(&pe, &image, base)? {
            let index = (0..TLS_SLOTS)
                .find(|&index| {
                    !self
                        .modules
                        .images
                        .values()
                        .any(|m| m.tls.as_ref().is_some_and(|t| t.index == index))
                })
                .ok_or_else(|| Failure::Expected(8, "static TLS slots exhausted".into()))?;
            image[(spec.index_va - base) as usize..(spec.index_va - base) as usize + 4]
                .copy_from_slice(&(index as u32).to_le_bytes());
            Some(StaticTls {
                index,
                template: spec.template,
                callbacks: spec.callbacks,
                blocks: HashMap::new(),
            })
        } else {
            None
        };
        image.resize(size as usize, 0);
        mem.map_image(base, size)
            .map_err(|e| Failure::Expected(e, "image mapping failed".into()))?;
        mem.write_bytes(base, &image).map_err(memory)?;
        let guest_path = fs.guest_path(&path);
        self.modules.paths.insert(path.clone(), base);
        self.modules.images.insert(
            base,
            Module {
                path: path.clone(),
                guest_path,
                size,
                entry: if pe.entry_rva == 0 {
                    0
                } else {
                    base + pe.entry_rva
                },
                exports: table,
                dependencies: Vec::new(),
                refs: 1,
                pinned: false,
                phase: Phase::Linking,
                tls,
                thread_calls_disabled: false,
            },
        );
        txn.created.push(base);
        for dll in imports {
            let dependency = self.link(&dll.dll, path.parent(), mem, txn, depth + 1)?;
            self.modules.attach_dependency(base, dependency, txn);
            for function in dll.functions {
                if function.iat_rva as u64 + 4 > pe.size_of_image as u64 {
                    return Err(bad("IAT out of image"));
                }
                let symbol = match function.name {
                    Some(name) => Symbol::Name(name),
                    None => Symbol::Ordinal(function.ordinal.unwrap()),
                };
                let target = self.resolve(
                    dependency,
                    symbol,
                    Some(base),
                    mem,
                    txn,
                    &mut HashSet::new(),
                )?;
                mem.write_u32(base + function.iat_rva, target)
                    .map_err(memory)?;
            }
        }
        mem.protect_image(base, &pe)
            .map_err(|e| Failure::Expected(e, "image protection failed".into()))?;
        self.modules.images.get_mut(&base).unwrap().phase = Phase::Linked;
        Ok(base)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve(
        &mut self,
        handle: u32,
        symbol: Symbol,
        owner: Option<u32>,
        mem: &mut GuestMem,
        txn: &mut Transaction,
        seen: &mut HashSet<(u32, String)>,
    ) -> LoadResult<u32> {
        let key = match &symbol {
            Symbol::Name(s) => s.clone(),
            Symbol::Ordinal(o) => format!("#{o}"),
        };
        if seen.len() >= 64 || !seen.insert((handle, key.clone())) {
            return Err(Failure::Expected(127, "export forwarder cycle".into()));
        }
        if let Some(dll) = self.modules.provider_name(handle) {
            if let Symbol::Name(name) = &symbol {
                if let Some(addr) = self
                    .api
                    .data_export(mem, dll, name, self.cmdline[0], self.cmdline[1])
                    .map_err(Failure::Fatal)?
                {
                    return Ok(addr);
                }
            }
            return match &symbol {
                Symbol::Name(name)
                    if crate::api::native_export(dll, name)
                        || self.api.lookup(dll, name).is_some() =>
                {
                    Ok(self.api.register(&dll.clone(), Some(name), None))
                }
                Symbol::Ordinal(ordinal)
                    if crate::win32::abi::ordinal_name(dll, *ordinal).is_some() =>
                {
                    Ok(self.api.register(&dll.clone(), None, Some(*ordinal)))
                }
                Symbol::Ordinal(ordinal) => self
                    .api
                    .entries
                    .iter()
                    .position(|e| &e.dll == dll && e.ordinal == Some(*ordinal))
                    .map(|i| crate::api::STUB_BASE + i as u32 * crate::api::STUB_STRIDE)
                    .ok_or_else(|| {
                        Failure::Expected(127, format!("native export {key} unavailable"))
                    }),
                _ => Err(Failure::Expected(
                    127,
                    format!("native export {key} unavailable"),
                )),
            };
        }
        let module = self
            .modules
            .images
            .get(&handle)
            .ok_or_else(|| Failure::Expected(6, "invalid module handle".into()))?;
        if module.phase == Phase::Abandoned {
            return Err(Failure::Expected(
                1114,
                "DLL initialization abandoned".into(),
            ));
        }
        let index = match &symbol {
            Symbol::Name(name) => module.exports.names.get(name).copied(),
            Symbol::Ordinal(ordinal) => (*ordinal as u32)
                .checked_sub(module.exports.ordinal_base)
                .map(|i| i as usize),
        };
        let export = index
            .and_then(|i| module.exports.functions.get(i))
            .cloned()
            .unwrap_or(Export::Missing);
        let parent = module.path.parent().map(Path::to_path_buf);
        match export {
            Export::Address(rva) => Ok(handle + rva),
            Export::Missing => Err(Failure::Expected(127, format!("export {key} not found"))),
            Export::Forward(text) => {
                let (dll, name) = text
                    .rsplit_once('.')
                    .ok_or_else(|| bad("invalid export forwarder"))?;
                let next = self.link(dll, parent.as_deref(), mem, txn, seen.len())?;
                if let Some(owner) = owner {
                    self.modules.attach_dependency(owner, next, txn);
                }
                let symbol = if let Some(ordinal) = name.strip_prefix('#') {
                    Symbol::Ordinal(
                        ordinal
                            .parse()
                            .map_err(|_| bad("invalid forwarded ordinal"))?,
                    )
                } else {
                    Symbol::Name(name.into())
                };
                self.resolve(next, symbol, owner, mem, txn, seen)
            }
        }
    }
}
