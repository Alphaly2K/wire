//! Win64 loaded images and logical providers own their names and lifetimes.
//! Guest callbacks, TLS allocation and detach remain process responsibilities.
use crate::{fs::GuestFs, x64::pe::Exports64};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub(crate) struct Modules {
    handles: BTreeMap<u64, (String, u32)>,
    pinned: BTreeSet<u64>,
    exports: BTreeMap<u64, Exports64>,
    paths: BTreeMap<u64, String>,
    images: BTreeMap<u64, (u32, u64)>,
    directory: Option<String>,
    next: u64,
}

/// Prepared before writing the guest output; committed only after that write.
pub(crate) struct ReferenceUpdate<'a> {
    base: u64,
    refs: &'a mut u32,
    next: u32,
    pinned: &'a mut BTreeSet<u64>,
    pin: bool,
}
impl ReferenceUpdate<'_> {
    pub(crate) fn commit(self) {
        *self.refs = self.next;
        if self.pin {
            self.pinned.insert(self.base);
        }
    }
}

pub(crate) enum Release {
    GuestReference,
    ProviderReference,
}

impl Modules {
    pub(crate) fn new(next: u64) -> Self {
        Self {
            handles: BTreeMap::new(),
            pinned: BTreeSet::new(),
            exports: BTreeMap::new(),
            paths: BTreeMap::new(),
            images: BTreeMap::new(),
            directory: None,
            next,
        }
    }
    pub(crate) fn register_guest(&mut self, name: &str, base: u64, exports: Exports64) {
        self.handles.insert(base, (name.to_ascii_lowercase(), 1));
        self.exports.insert(base, exports);
    }
    pub(crate) fn register_path(&mut self, base: u64, path: String) {
        self.paths.insert(base, path);
    }
    pub(crate) fn register_image(&mut self, base: u64, size: u32, entry: u64) {
        self.images.insert(base, (size, entry));
    }
    pub(crate) fn register_provider(&mut self, name: &str) {
        if self
            .handles
            .values()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            return;
        }
        self.allocate_provider(name.to_ascii_lowercase());
    }
    fn allocate_provider(&mut self, name: String) -> u64 {
        let handle = self.next;
        self.next += 0x10000;
        self.handles.insert(handle, (name, 1));
        handle
    }
    pub(crate) fn name(&self, handle: u64) -> Option<&String> {
        self.handles.get(&handle).map(|(name, _)| name)
    }
    pub(crate) fn contains(&self, handle: u64) -> bool {
        self.handles.contains_key(&handle)
    }
    pub(crate) fn bases(&self) -> impl Iterator<Item = u64> + '_ {
        self.handles.keys().copied()
    }
    pub(crate) fn exports(&self, handle: u64) -> Option<&Exports64> {
        self.exports.get(&handle)
    }
    pub(crate) fn path(&self, handle: u64) -> Option<&String> {
        self.paths.get(&handle)
    }
    pub(crate) fn image(&self, handle: u64) -> Option<(u32, u64)> {
        self.images.get(&handle).copied()
    }
    pub(crate) fn containing_image(&self, address: u64) -> Option<u64> {
        self.images
            .iter()
            .find(|&(&base, &(size, _))| address >= base && address - base < size as u64)
            .map(|(&base, _)| base)
    }
    pub(crate) fn pin(&mut self, handle: u64) {
        self.pinned.insert(handle);
    }
    pub(crate) fn prepare_reference(
        &mut self,
        base: u64,
        flags: u64,
    ) -> Result<ReferenceUpdate<'_>, String> {
        let refs = &mut self
            .handles
            .get_mut(&base)
            .ok_or("invalid Win64 module handle")?
            .1;
        let next = if flags & 2 != 0 {
            *refs
        } else {
            refs.checked_add(1).ok_or("module reference overflow")?
        };
        Ok(ReferenceUpdate {
            base,
            refs,
            next,
            pinned: &mut self.pinned,
            pin: flags & 1 != 0,
        })
    }
    pub(crate) fn release(&mut self, handle: u64) -> Result<Release, String> {
        if self.exports.contains_key(&handle) {
            let refs = &mut self.handles.get_mut(&handle).unwrap().1;
            if *refs <= 1 && !self.pinned.contains(&handle) {
                return Err("Win64 guest DLL detach/unload not implemented".into());
            }
            *refs = refs.saturating_sub(1).max(1);
            return Ok(Release::GuestReference);
        }
        let Some((module, refs)) = self.handles.get_mut(&handle) else {
            return Err("invalid Win64 module handle".into());
        };
        if (module != "kernel32.dll" && !self.pinned.contains(&handle)) || *refs > 1 {
            *refs -= 1;
            if *refs == 0 {
                self.handles.remove(&handle);
            }
        }
        Ok(Release::ProviderReference)
    }
    pub(crate) fn native_module(
        &mut self,
        fs: Option<&GuestFs>,
        name: &str,
        load: bool,
    ) -> Result<u64, String> {
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
            let path = fs.and_then(|fs| fs.resolve(name).ok());
            let loaded = self
                .paths
                .values()
                .any(|p| path.is_some() && fs.and_then(|fs| fs.resolve(p).ok()) == path);
            // Only the canonical system32 path names a logical provider.
            let system_provider = fs.zip(path.as_ref()).is_some_and(|(fs, path)| {
                (dynamic_provider(&module)
                    || self.handles.iter().any(|(&handle, (name, _))| {
                        name == &module && !self.exports.contains_key(&handle)
                    }))
                    && fs
                        .guest_path(path)
                        .eq_ignore_ascii_case(&format!("C:\\Windows\\System32\\{module}"))
            });
            if !loaded && !system_provider {
                return Err(format!("Win64 guest DLL requires process loader: {name}"));
            }
        }
        if let Some((&handle, (_, refs))) = self.handles.iter_mut().find(|(_, (n, _))| n == &module)
        {
            if load {
                *refs = refs.checked_add(1).ok_or("module reference overflow")?;
            }
            return Ok(handle);
        }
        if !load {
            return Ok(0);
        }
        if !dynamic_provider(&module) {
            return Err(format!("Win64 guest DLL loading not implemented: {module}"));
        }
        Ok(self.allocate_provider(module))
    }
    pub(crate) fn directory(&self) -> Option<&str> {
        self.directory.as_deref()
    }
    pub(crate) fn set_directory(&mut self, directory: Option<String>) {
        self.directory = directory;
    }
    pub(crate) fn absolute_path(name: &str) -> bool {
        let bytes = name.as_bytes();
        bytes.first() == Some(&b'/')
            || bytes.first() == Some(&b'\\')
            || (bytes.get(1) == Some(&b':') && matches!(bytes.get(2), Some(b'/' | b'\\')))
    }
    /// Confined existing search order; PATH/AddDllDirectory are still absent.
    pub(crate) fn resolve_path(
        &self,
        fs: &GuestFs,
        exe_path: &str,
        name: &str,
        flags: u64,
        altered_directory: Option<&str>,
    ) -> Result<PathBuf, u32> {
        let filename = name.replace('/', "\\");
        let basename = filename.rsplit('\\').next().unwrap_or(&filename);
        let filename = if basename.contains('.') {
            filename
        } else {
            format!("{filename}.dll")
        };
        if filename.contains(['\\', ':']) {
            let path = fs.resolve(&filename)?;
            return path.is_file().then_some(path).ok_or(126);
        }
        let mut directories = Vec::new();
        if flags & 0x1000 != 0 || flags & 0x800 == 0 {
            let directory = if let Some(directory) = altered_directory {
                directory
            } else {
                let end = exe_path.rfind(['\\', '/']).ok_or(123u32)?;
                &exe_path[..end]
            };
            directories.push(directory.to_owned());
            if let Some(dir) = self.directory.as_ref().filter(|d| !d.is_empty()) {
                directories.push(dir.clone());
            }
        }
        directories.push("C:\\Windows\\System32".into());
        if flags == 0 || flags == 8 {
            directories.push("C:\\Windows".into());
            if self.directory.is_none() {
                directories.push(fs.cwd());
            }
        }
        for dir in directories {
            let path = fs.resolve(&format!("{dir}\\{filename}"))?;
            if path.is_file() {
                return Ok(path);
            }
        }
        Err(126)
    }
}

/// Legacy built-in eligibility, separate from guest image discovery.
fn dynamic_provider(name: &str) -> bool {
    matches!(
        name,
        "kernel32.dll"
            | "kernelbase.dll"
            | "hid.dll"
            | "setupapi.dll"
            | "secur32.dll"
            | "ws2_32.dll"
            | "oleaut32.dll"
            | "winhttp.dll"
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
