//! Guest PE32 DLL mapping, linking and initialization. Never maps host code.
use crate::mem::GuestMem;
use crate::pe::{PeFile, DIR_BASERELOC, DIR_EXPORT};
pub use crate::x86::process::module_loading::dispatch;
pub use crate::x86::process::module_notifications::{notify_process_exit, notify_thread};
use crate::x86::process::GuestCallOutcome;
use std::collections::HashMap;
use std::path::PathBuf;
mod linker;
pub(crate) use linker::Linker;

const ARENA_START: u32 = 0x5000_0000;
const ARENA_END: u32 = 0x6800_0000;
const GRANULARITY: u32 = 0x10000;
const TLS_SLOTS: usize = 64;

struct StaticTls {
    index: usize,
    template: Vec<u8>,
    callbacks: Vec<u32>,
    blocks: HashMap<u32, u32>, // TEB -> per-thread copy
}

#[derive(Clone, Debug)]
enum Export {
    Missing,
    Address(u32),
    Forward(String),
}
#[derive(Default)]
struct Exports {
    ordinal_base: u32,
    functions: Vec<Export>,
    names: HashMap<String, usize>,
}
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Linking,
    Linked,
    Initializing,
    Ready,
    Unloading,
    Abandoned,
}
struct Module {
    path: PathBuf,
    guest_path: String,
    size: u32,
    entry: u32,
    exports: Exports,
    dependencies: Vec<u32>,
    refs: u32,
    pinned: bool,
    phase: Phase,
    tls: Option<StaticTls>,
    thread_calls_disabled: bool,
}
#[derive(Default)]
pub struct Modules {
    providers: crate::win32::x86::modules::Providers,
    images: HashMap<u32, Module>,
    paths: HashMap<PathBuf, u32>,
    tls_vectors: HashMap<u32, u32>, // TEB -> ThreadLocalStoragePointer
    init_order: Vec<u32>,
    process_detaching: bool,
}

pub(crate) enum Failure {
    Expected(u32, String),
    Fatal(String),
    Control(GuestCallOutcome),
}
pub(crate) type LoadResult<T> = Result<T, Failure>;
fn bad(message: impl Into<String>) -> Failure {
    Failure::Expected(193, message.into())
}
pub(crate) fn memory(error: impl std::fmt::Display) -> Failure {
    Failure::Fatal(error.to_string())
}
fn read32(bytes: &[u8], offset: usize) -> LoadResult<u32> {
    bytes
        .get(offset..offset + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| bad("PE DWORD out of image"))
}
fn cstring(bytes: &[u8], offset: usize, end: usize) -> LoadResult<String> {
    let slice = bytes
        .get(offset..end.min(bytes.len()))
        .ok_or_else(|| bad("PE string out of image"))?;
    let len = slice
        .iter()
        .take(1024)
        .position(|&b| b == 0)
        .ok_or_else(|| bad("unterminated PE string"))?;
    String::from_utf8(slice[..len].to_vec()).map_err(|_| bad("non-UTF8 PE export string"))
}
fn exports(pe: &PeFile, image: &[u8]) -> LoadResult<Exports> {
    let dir = pe.dir(DIR_EXPORT);
    if dir.rva == 0 {
        return Ok(Exports::default());
    }
    let start = dir.rva as usize;
    let end = start
        .checked_add(dir.size as usize)
        .filter(|&e| e <= image.len() && e >= start + 40)
        .ok_or_else(|| bad("invalid export directory"))?;
    let ordinal_base = read32(image, start + 16)?;
    let count = read32(image, start + 20)? as usize;
    let names = read32(image, start + 24)? as usize;
    if count > 65536 || names > 65536 {
        return Err(bad("export table too large"));
    }
    let functions = read32(image, start + 28)? as usize;
    let name_table = read32(image, start + 32)? as usize;
    let ordinals = read32(image, start + 36)? as usize;
    let mut result = Exports {
        ordinal_base,
        ..Exports::default()
    };
    for index in 0..count {
        let rva = read32(image, functions + index * 4)?;
        result.functions.push(if rva == 0 {
            Export::Missing
        } else if (dir.rva as u64..dir.rva as u64 + dir.size as u64).contains(&(rva as u64)) {
            Export::Forward(cstring(image, rva as usize, end)?)
        } else if rva as usize >= image.len() {
            return Err(bad("export target out of image"));
        } else {
            Export::Address(rva)
        });
    }
    for index in 0..names {
        let rva = read32(image, name_table + index * 4)? as usize;
        let bytes = image
            .get(ordinals + index * 2..ordinals + index * 2 + 2)
            .ok_or_else(|| bad("export ordinal table out of image"))?;
        let ordinal = u16::from_le_bytes(bytes.try_into().unwrap()) as usize;
        if ordinal >= count {
            return Err(bad("export name ordinal out of range"));
        }
        let name = cstring(image, rva, image.len())?;
        if result.names.insert(name, ordinal).is_some() {
            return Err(bad("duplicate export name"));
        }
    }
    Ok(result)
}
fn prepare(bytes: &[u8]) -> LoadResult<(PeFile, Vec<u8>, Exports)> {
    let pe = PeFile::parse(bytes).map_err(bad)?;
    if pe.characteristics & 0x2000 == 0
        || pe.size_of_image == 0
        || pe.size_of_image > ARENA_END - ARENA_START
        || pe.entry_rva >= pe.size_of_image
        || pe.size_of_headers > pe.size_of_image
        || pe.size_of_headers as usize > bytes.len()
    {
        return Err(bad("invalid PE32 DLL layout"));
    }
    if pe.dir(13).rva != 0 {
        return Err(bad("delay imports are not supported yet"));
    }
    // Some MinGW images end at the last virtual byte rather than at a page
    // boundary. File-alignment padding may still occupy that last mapped page.
    let mapped_size = pe
        .size_of_image
        .checked_add(4095)
        .map(|n| n & !4095)
        .ok_or_else(|| bad("DLL image size overflow"))?;
    let mut image = vec![0; mapped_size as usize];
    image[..pe.size_of_headers as usize].copy_from_slice(&bytes[..pe.size_of_headers as usize]);
    let mut ranges = vec![(0, pe.size_of_headers)];
    for section in &pe.sections {
        let span = section.virtual_size.max(section.raw_size);
        let virtual_span = if section.virtual_size == 0 {
            section.raw_size
        } else {
            section.virtual_size
        };
        if section.virtual_address as u64 + virtual_span as u64 > pe.size_of_image as u64 {
            return Err(bad("virtual section out of image"));
        }
        let end = section
            .virtual_address
            .checked_add(span)
            .filter(|&e| e <= mapped_size)
            .ok_or_else(|| bad("section out of image"))?;
        if span != 0
            && ranges
                .iter()
                .any(|&(start, stop)| section.virtual_address < stop && end > start)
        {
            return Err(bad("overlapping image sections"));
        }
        ranges.push((section.virtual_address, end));
        if section.raw_size != 0 {
            let raw = section.raw_ptr as usize;
            let data = bytes
                .get(raw..raw + section.raw_size as usize)
                .ok_or_else(|| bad("truncated DLL section"))?;
            image[section.virtual_address as usize..section.virtual_address as usize + data.len()]
                .copy_from_slice(data);
        }
    }
    for (index, dir) in pe.dirs.iter().enumerate() {
        if index != 4
            && dir.rva != 0
            && (dir.rva as u64 + dir.size as u64 > pe.size_of_image as u64)
        {
            return Err(bad("data directory out of image"));
        }
    }
    let table = exports(&pe, &image)?;
    Ok((pe, image, table))
}
fn relocate(pe: &PeFile, image: &mut [u8], base: u32) -> LoadResult<()> {
    if base == pe.image_base {
        return Ok(());
    }
    let dir = pe.dir(DIR_BASERELOC);
    if pe.characteristics & 1 != 0 {
        return Err(bad("DLL cannot be relocated"));
    }
    // An absent relocation directory means there are no absolute fixups.
    // Resource-only / position-independent DLLs need no patches. RELOCS_STRIPPED
    // above still rejects fixed images, matching Hangover Wine's loader.
    if dir.size == 0 {
        return Ok(());
    }
    if dir.rva == 0 {
        return Err(bad("invalid relocation directory"));
    }
    let mut at = dir.rva as usize;
    let end = at + dir.size as usize;
    let delta = base.wrapping_sub(pe.image_base);
    let mut patches = Vec::new();
    while at < end {
        if end - at < 8 {
            return Err(bad("truncated relocation block"));
        }
        let page = read32(image, at)?;
        let size = read32(image, at + 4)? as usize;
        if size < 8 || !size.is_multiple_of(2) || at + size > end {
            return Err(bad("invalid relocation block size"));
        }
        for pos in (at + 8..at + size).step_by(2) {
            let fixup = u16::from_le_bytes(image[pos..pos + 2].try_into().unwrap());
            match fixup >> 12 {
                0 => {}
                3 => {
                    let target = page
                        .checked_add((fixup & 0xfff) as u32)
                        .ok_or_else(|| bad("relocation RVA overflow"))?
                        as usize;
                    let old = read32(image, target)?;
                    patches.push((target, old.wrapping_add(delta)));
                }
                _ => return Err(bad("unsupported PE32 relocation type")),
            }
        }
        at += size;
    }
    // Validate all blocks before altering the relocation directory itself.
    for (offset, value) in patches {
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}
struct TlsSpec {
    index_va: u32,
    template: Vec<u8>,
    callbacks: Vec<u32>,
}
// TLS fields are relocated VAs. Callback tables and targets remain bounded
// guest data/code, and callbacks execute through the ordinary checked CPU.
fn static_tls(pe: &PeFile, image: &[u8], base: u32) -> LoadResult<Option<TlsSpec>> {
    let dir = pe.dir(9);
    if dir.rva == 0 {
        return Ok(None);
    }
    if dir.size < 24 {
        return Err(bad("truncated TLS directory"));
    }
    let at = dir.rva as usize;
    let start = read32(image, at)?;
    let end = read32(image, at + 4)?;
    let index = read32(image, at + 8)?;
    let callbacks = read32(image, at + 12)?;
    let zero = read32(image, at + 16)?;
    let alignment = (read32(image, at + 20)? >> 20) & 15;
    if alignment > 5 {
        return Err(bad("TLS alignment exceeds 16 bytes"));
    }
    let offset = |va: u32, length: u32| -> LoadResult<usize> {
        va.checked_sub(base)
            .filter(|&rva| rva as u64 + length as u64 <= pe.size_of_image as u64)
            .map(|rva| rva as usize)
            .ok_or_else(|| bad("TLS VA out of image"))
    };
    offset(index, 4)?;
    let mut functions = Vec::new();
    if callbacks != 0 {
        let mut table = callbacks;
        loop {
            let target = read32(image, offset(table, 4)?)?;
            if target == 0 {
                break;
            }
            if functions.len() >= 256 {
                return Err(bad("TLS callback table too large"));
            }
            let rva = offset(target, 1)? as u32;
            if !pe
                .sections
                .iter()
                .any(|s| s.is_executable() && s.contains_rva(rva))
            {
                return Err(bad("TLS callback target is not image code"));
            }
            functions.push(target);
            table = table
                .checked_add(4)
                .ok_or_else(|| bad("TLS callback table overflow"))?;
        }
    }
    let length = end
        .checked_sub(start)
        .ok_or_else(|| bad("invalid TLS template range"))?;
    let source = if start == 0 && end == 0 {
        0
    } else {
        offset(start, length)?
    };
    let size = length
        .checked_add(zero)
        .filter(|&n| n <= 16 * 1024 * 1024)
        .ok_or_else(|| bad("TLS template too large"))?;
    let mut template = vec![0; size as usize];
    template[..length as usize].copy_from_slice(&image[source..source + length as usize]);
    Ok(Some(TlsSpec {
        index_va: index,
        template,
        callbacks: functions,
    }))
}
fn basename(name: &str) -> String {
    name.rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_lowercase()
}
fn library_name(name: &str) -> String {
    let mut result = name.to_string();
    if !basename(name).contains('.') {
        result.push_str(".dll");
    }
    result
}
pub fn native_module(name: &str) -> bool {
    matches!(
        name,
        "kernel32.dll"
            | "ntdll.dll"
            | "user32.dll"
            | "gdi32.dll"
            | "gdiplus.dll"
            | "d3d9.dll"
            | "msvcrt.dll"
            | "advapi32.dll"
            | "version.dll"
            | "winmm.dll"
            | "imm32.dll"
            | "ole32.dll"
            | "shell32.dll"
            | "comctl32.dll"
            | "shlwapi.dll"
            | "ws2_32.dll"
            | "mpr.dll"
            | "comdlg32.dll"
            | "mfplat.dll"
            | "mf.dll"
            | "ddraw.dll"
            | "dsound.dll"
            | "dinput.dll"
            | "oleaut32.dll"
            | "wininet.dll"
            | "riched32.dll"
            | "riched20.dll"
            | "msftedit.dll"
            | "imagehlp.dll"
    )
}
pub(crate) struct Initialization {
    pub dependencies: Vec<u32>,
    pub entry: u32,
    pub path: PathBuf,
}
pub(crate) struct RetiredImage {
    pub size: u32,
    pub dependencies: Vec<u32>,
}
impl Modules {
    pub(crate) fn disable_thread_calls(&mut self, handle: u32) -> LoadResult<u32> {
        if self.provider_name(handle).is_some() {
            return Ok(1);
        }
        let module = self
            .images
            .get_mut(&handle)
            .ok_or_else(|| Failure::Expected(126, "module is not loaded".into()))?;
        if module.tls.is_some() {
            return Err(Failure::Expected(
                126,
                "static TLS requires thread notifications".into(),
            ));
        }
        module.thread_calls_disabled = true;
        Ok(1)
    }
    pub(crate) fn lookup_handle(&self, name: &str, fs: Option<&crate::fs::GuestFs>) -> Option<u32> {
        self.handle(name, fs)
            .or_else(|| self.provider_handle(&basename(&library_name(name))))
    }
    pub(crate) fn pin_image(&mut self, handle: u32) {
        if let Some(module) = self.images.get_mut(&handle) {
            module.pinned = true;
        }
    }
    pub(crate) fn guest_image_path(&self, handle: u32) -> Option<&String> {
        self.images.get(&handle).map(|m| &m.guest_path)
    }
    pub(crate) fn initialized_entry(&self, handle: u32) -> u32 {
        self.images[&handle].entry
    }
    pub(crate) fn rollback_references(&mut self, txn: &Transaction) {
        for &(owner, dependency) in txn.edges.iter().rev() {
            if let Some(module) = self.images.get_mut(&owner) {
                module.dependencies.retain(|&h| h != dependency);
            }
        }
        for &handle in txn.acquired.iter().rev() {
            if let Some(module) = self.images.get_mut(&handle) {
                module.refs -= 1;
            }
        }
    }
    pub(crate) fn rollback_image(
        &mut self,
        mem: &mut GuestMem,
        heap: &mut crate::heap::GuestHeap,
        process_heap: u32,
        handle: u32,
    ) -> Result<Option<u32>, String> {
        if let Some(mut module) = self.images.remove(&handle) {
            self.release_module_tls(&mut module, mem, heap, process_heap)?;
            self.paths.remove(&module.path);
            self.init_order.retain(|&h| h != handle);
            mem.decommit_image(handle, module.size)
                .map_err(|e| e.to_string())?;
            return Ok(Some(module.size));
        }
        Ok(None)
    }
    pub(crate) fn begin_initialization(
        &mut self,
        handle: u32,
    ) -> LoadResult<Option<Initialization>> {
        let Some(module) = self.images.get_mut(&handle) else {
            return Ok(None);
        };
        if matches!(module.phase, Phase::Initializing | Phase::Ready) {
            return Ok(None);
        }
        if module.phase != Phase::Linked {
            return Err(bad("module graph not fully linked"));
        }
        module.phase = Phase::Initializing;
        let dependencies = module.dependencies.clone();
        let entry = module.entry;
        let path = module.path.clone();
        Ok(Some(Initialization {
            dependencies,
            entry,
            path,
        }))
    }
    pub(crate) fn complete_initialization(&mut self, handle: u32, txn: &mut Transaction) {
        self.images.get_mut(&handle).unwrap().phase = Phase::Ready;
        txn.record_initialization(handle);
        self.init_order.push(handle);
    }
    pub(crate) fn begin_unload(&mut self, handle: u32) -> LoadResult<Option<u32>> {
        if self.provider_name(handle).is_some() {
            return Ok(None);
        }
        let module = self
            .images
            .get_mut(&handle)
            .ok_or_else(|| Failure::Expected(6, "invalid module handle".into()))?;
        if module.pinned {
            return Ok(None);
        }
        if module.phase != Phase::Ready {
            return Err(Failure::Expected(
                6,
                "module is not ready for unload".into(),
            ));
        }
        if module.refs > 1 {
            module.refs -= 1;
            return Ok(None);
        }
        module.phase = Phase::Unloading;
        let entry = module.entry;
        Ok(Some(entry))
    }
    pub(crate) fn finish_unload(
        &mut self,
        mem: &mut GuestMem,
        heap: &mut crate::heap::GuestHeap,
        process_heap: u32,
        handle: u32,
    ) -> LoadResult<RetiredImage> {
        let mut module = self.images.remove(&handle).unwrap();
        self.release_module_tls(&mut module, mem, heap, process_heap)
            .map_err(Failure::Fatal)?;
        self.paths.remove(&module.path);
        self.init_order.retain(|&h| h != handle);
        mem.decommit_image(handle, module.size).map_err(memory)?;
        Ok(RetiredImage {
            size: module.size,
            dependencies: module.dependencies,
        })
    }
    pub(crate) fn tls_callbacks(&self, handle: u32) -> Vec<u32> {
        self.images
            .get(&handle)
            .and_then(|m| m.tls.as_ref())
            .map(|t| t.callbacks.clone())
            .unwrap_or_default()
    }
    pub(crate) fn image_present(&self, handle: u32) -> bool {
        self.images.contains_key(&handle)
    }
    pub(crate) fn notification_entry(&self, handle: u32) -> u32 {
        self.images.get(&handle).map_or(0, |m| m.entry)
    }
    pub(crate) fn process_detaching(&self) -> bool {
        self.process_detaching
    }
    pub(crate) fn begin_process_detach(&mut self) {
        self.process_detaching = true;
    }
    pub(crate) fn notification_order(&self) -> Vec<u32> {
        self.init_order.clone()
    }
    pub(crate) fn thread_notification_enabled(&self, handle: u32) -> bool {
        self.images
            .get(&handle)
            .is_some_and(|m| m.phase == Phase::Ready && !m.thread_calls_disabled)
    }
    pub(crate) fn notification_ready(&self, handle: u32) -> bool {
        self.images
            .get(&handle)
            .is_some_and(|m| m.phase == Phase::Ready)
    }
    /// Existing logical provider handles are persistent for the process.
    pub fn provider_handle_for(&mut self, dll: &str) -> u32 {
        self.providers.handle_for(dll)
    }
    /// Lookup uses the existing lowercase key; it never registers a provider.
    pub fn provider_handle(&self, dll: &str) -> Option<u32> {
        self.providers.handle(dll)
    }
    pub fn provider_name(&self, handle: u32) -> Option<&String> {
        self.providers.name(handle)
    }
    pub fn register_main(
        &mut self,
        mem: &GuestMem,
        base: u32,
        path: PathBuf,
        guest_path: String,
    ) -> Result<(), String> {
        let result = (|| -> LoadResult<()> {
            let mut headers = vec![0; 0x1000];
            mem.read_bytes(base, &mut headers).map_err(memory)?;
            let pe = PeFile::parse(&headers).map_err(bad)?;
            if pe.size_of_image == 0 || pe.size_of_image > 64 * 1024 * 1024 {
                return Err(bad("main image too large"));
            }
            let image = mem
                .image_snapshot(base, pe.size_of_image)
                .map_err(|e| bad(format!("main image snapshot: error {e}")))?;
            let table = exports(&pe, &image)?;
            let tls = if let Some(spec) = static_tls(&pe, &image, base)? {
                let index = (0..TLS_SLOTS)
                    .find(|&index| {
                        !self
                            .images
                            .values()
                            .any(|m| m.tls.as_ref().is_some_and(|t| t.index == index))
                    })
                    .ok_or_else(|| Failure::Expected(8, "static TLS slots exhausted".into()))?;
                mem.write_u32(spec.index_va, index as u32).map_err(memory)?;
                Some(StaticTls {
                    index,
                    template: spec.template,
                    callbacks: spec.callbacks,
                    blocks: HashMap::new(),
                })
            } else {
                None
            };
            self.paths.insert(path.clone(), base);
            self.images.insert(
                base,
                Module {
                    path,
                    guest_path,
                    size: pe.size_of_image,
                    entry: 0,
                    exports: table,
                    dependencies: Vec::new(),
                    refs: 1,
                    pinned: true,
                    phase: Phase::Ready,
                    tls,
                    thread_calls_disabled: false,
                },
            );
            self.init_order.push(base);
            Ok(())
        })();
        result.map_err(|e| match e {
            Failure::Expected(_, msg) | Failure::Fatal(msg) => msg,
            _ => unreachable!(),
        })
    }
    fn handle(&self, name: &str, fs: Option<&crate::fs::GuestFs>) -> Option<u32> {
        if name.contains(['\\', '/', ':']) {
            let path = fs?.resolve(&library_name(name)).ok()?.canonicalize().ok()?;
            return self
                .paths
                .get(&path)
                .copied()
                .filter(|h| self.images[h].phase != Phase::Abandoned);
        }
        let key = basename(&library_name(name));
        self.images
            .iter()
            .filter(|(_, module)| {
                module.phase != Phase::Abandoned && basename(&module.guest_path) == key
            })
            .min_by_key(|(&h, _)| h)
            .map(|(&h, _)| h)
    }
    pub(crate) fn address(&self, address: u32) -> Option<u32> {
        self.images
            .iter()
            .find(|(&base, module)| {
                module.phase != Phase::Abandoned
                    && address >= base
                    && (address as u64) < base as u64 + module.size as u64
            })
            .map(|(&h, _)| h)
    }
    /// Populate FS:[0x2c] before a newly created thread executes guest code.
    pub fn setup_tls_thread(
        &mut self,
        mem: &mut GuestMem,
        heap: &mut crate::heap::GuestHeap,
        process_heap: u32,
        teb: u32,
    ) -> Result<(), String> {
        if !self.images.values().any(|m| m.tls.is_some()) {
            return Ok(());
        }
        let vector = if let Some(&vector) = self.tls_vectors.get(&teb) {
            vector
        } else {
            let vector = heap.alloc(mem, process_heap, TLS_SLOTS as u32 * 4, true)?;
            if vector == 0 {
                return Err("cannot allocate static TLS vector".into());
            }
            mem.write_u32(teb + 0x2c, vector)
                .map_err(|e| e.to_string())?;
            self.tls_vectors.insert(teb, vector);
            vector
        };
        for module in self.images.values_mut() {
            let Some(tls) = &mut module.tls else { continue };
            if tls.blocks.contains_key(&teb) {
                continue;
            }
            let block = heap.alloc(mem, process_heap, tls.template.len() as u32, false)?;
            if block == 0 {
                return Err("cannot allocate static TLS block".into());
            }
            tls.blocks.insert(teb, block);
            mem.write_bytes(block, &tls.template)
                .map_err(|e| e.to_string())?;
            mem.write_u32(vector + tls.index as u32 * 4, block)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn release_tls_thread(
        &mut self,
        heap: &mut crate::heap::GuestHeap,
        process_heap: u32,
        teb: u32,
    ) {
        for module in self.images.values_mut() {
            if let Some(tls) = &mut module.tls {
                if let Some(block) = tls.blocks.remove(&teb) {
                    heap.free(process_heap, block);
                }
            }
        }
        if let Some(vector) = self.tls_vectors.remove(&teb) {
            heap.free(process_heap, vector);
        }
    }
    fn release_module_tls(
        &mut self,
        module: &mut Module,
        mem: &GuestMem,
        heap: &mut crate::heap::GuestHeap,
        process_heap: u32,
    ) -> Result<(), String> {
        if let Some(tls) = module.tls.take() {
            for (teb, block) in tls.blocks {
                if let Some(&vector) = self.tls_vectors.get(&teb) {
                    mem.write_u32(vector + tls.index as u32 * 4, 0)
                        .map_err(|e| e.to_string())?;
                }
                heap.free(process_heap, block);
            }
        }
        Ok(())
    }
    fn allocate(&self, pe: &PeFile, mem: &GuestMem) -> LoadResult<(u32, u32)> {
        let size = pe
            .size_of_image
            .checked_add(GRANULARITY - 1)
            .map(|n| n & !(GRANULARITY - 1))
            .ok_or_else(|| bad("DLL image size overflow"))?;
        let free = |base: u32| {
            base >= ARENA_START
                && base.is_multiple_of(GRANULARITY)
                && base as u64 + size as u64 <= ARENA_END as u64
                && (base..base + size)
                    .step_by(mem.host_page_size() as usize)
                    .all(|p| !mem.is_committed(p))
        };
        if free(pe.image_base) {
            return Ok((pe.image_base, size));
        }
        (ARENA_START..ARENA_END)
            .step_by(GRANULARITY as usize)
            .find(|&base| free(base))
            .map(|base| (base, size))
            .ok_or_else(|| Failure::Expected(8, "DLL arena exhausted".into()))
    }
}

#[derive(Default)]
pub(crate) struct Transaction {
    created: Vec<u32>,
    acquired: Vec<u32>,
    initialized: Vec<u32>,
    edges: Vec<(u32, u32)>,
}
impl Transaction {
    pub(crate) fn created(&self) -> &[u32] {
        &self.created
    }
    pub(crate) fn initialized(&self) -> &[u32] {
        &self.initialized
    }
    pub(crate) fn record_initialization(&mut self, handle: u32) {
        self.initialized.push(handle);
    }
}
#[derive(Clone, Debug)]
pub(crate) enum Symbol {
    Name(String),
    Ordinal(u16),
}
impl Modules {
    pub(crate) fn abandon(&mut self, txn: &Transaction) {
        for &handle in &txn.created {
            if let Some(module) = self.images.get_mut(&handle) {
                module.phase = Phase::Abandoned;
            }
        }
    }
    fn attach_dependency(&mut self, owner: u32, dependency: u32, txn: &mut Transaction) {
        if owner == dependency || self.images[&owner].dependencies.contains(&dependency) {
            // One reference per graph edge, including repeated forwarded lookups.
            if let Some(index) = txn.acquired.iter().rposition(|&h| h == dependency) {
                txn.acquired.remove(index);
                self.images.get_mut(&dependency).unwrap().refs -= 1;
            }
        } else {
            self.images
                .get_mut(&owner)
                .unwrap()
                .dependencies
                .push(dependency);
            txn.edges.push((owner, dependency));
        }
    }
    pub(crate) fn acquire(&mut self, txn: &mut Transaction, handle: u32) -> LoadResult<()> {
        if let Some(module) = self.images.get_mut(&handle) {
            if matches!(module.phase, Phase::Unloading | Phase::Abandoned) {
                return Err(Failure::Expected(126, "module unloading".into()));
            }
            if !module.pinned {
                module.refs = module
                    .refs
                    .checked_add(1)
                    .ok_or_else(|| bad("DLL refcount overflow"))?;
                txn.acquired.push(handle);
            }
        }
        Ok(())
    }
}
