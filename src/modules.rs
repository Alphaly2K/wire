//! Guest PE32 DLL mapping, linking and initialization. Never maps host code.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome};
use crate::mem::GuestMem;
use crate::pe::{PeFile, DIR_BASERELOC, DIR_EXPORT};
use crate::thread::Sched;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    images: HashMap<u32, Module>,
    paths: HashMap<PathBuf, u32>,
    tls_vectors: HashMap<u32, u32>, // TEB -> ThreadLocalStoragePointer
    init_order: Vec<u32>,
    process_detaching: bool,
}

enum Failure {
    Expected(u32, String),
    Fatal(String),
    Control(GuestCallOutcome),
}
type LoadResult<T> = Result<T, Failure>;
fn bad(message: impl Into<String>) -> Failure {
    Failure::Expected(193, message.into())
}
fn memory(error: impl std::fmt::Display) -> Failure {
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
impl Modules {
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
    fn address(&self, address: u32) -> Option<u32> {
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
struct Transaction {
    created: Vec<u32>,
    acquired: Vec<u32>,
    initialized: Vec<u32>,
    edges: Vec<(u32, u32)>,
}
#[derive(Clone, Debug)]
enum Symbol {
    Name(String),
    Ordinal(u16),
}
fn abandon(state: &mut ApiState, txn: &Transaction) {
    for &handle in &txn.created {
        if let Some(module) = state.modules.images.get_mut(&handle) {
            module.phase = Phase::Abandoned;
        }
    }
}
fn attach_dependency(owner: u32, dependency: u32, state: &mut ApiState, txn: &mut Transaction) {
    if owner == dependency
        || state.modules.images[&owner]
            .dependencies
            .contains(&dependency)
    {
        // One reference per graph edge, including repeated forwarded lookups.
        if let Some(index) = txn.acquired.iter().rposition(|&h| h == dependency) {
            txn.acquired.remove(index);
            state.modules.images.get_mut(&dependency).unwrap().refs -= 1;
        }
    } else {
        state
            .modules
            .images
            .get_mut(&owner)
            .unwrap()
            .dependencies
            .push(dependency);
        txn.edges.push((owner, dependency));
    }
}
#[allow(clippy::too_many_arguments)]
fn link(
    name: &str,
    parent: Option<&Path>,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    txn: &mut Transaction,
    depth: usize,
) -> LoadResult<u32> {
    if depth > 64 {
        return Err(bad("DLL dependency depth exceeded"));
    }
    let name = library_name(name);
    let short = basename(&name);
    if native_module(&short) {
        let handle = state.module_handle_for(&short);
        let classes: &[(&str, bool)] = match short.as_str() {
            "riched32.dll" => &[("RichEdit", false)],
            "riched20.dll" => &[("RichEdit20A", false), ("RichEdit20W", true)],
            "msftedit.dll" => &[("RICHEDIT50W", true)],
            _ => &[],
        };
        for &(name, wide) in classes {
            crate::user32::register_system_class(mem, api, state, handle, name, wide)
                .map_err(Failure::Fatal)?;
        }
        return Ok(handle);
    }
    // Consult the loaded module list before searching the filesystem.
    if !name.contains(['\\', '/', ':']) {
        if let Some(handle) = state.modules.handle(&name, state.fs.as_ref()) {
            acquire(state, txn, handle)?;
            return Ok(handle);
        }
    }
    let fs = state
        .fs
        .as_ref()
        .ok_or_else(|| Failure::Expected(126, "guest filesystem unavailable".into()))?;
    let mut candidates = Vec::new();
    if !name.contains(['\\', '/', ':']) {
        if let Some(parent) = parent {
            candidates.push(parent.join(&name).to_string_lossy().into_owned());
        }
        let exe_dir = state.exe_path.rsplit_once('\\').map_or("C:\\", |(p, _)| p);
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
    if let Some(&handle) = state.modules.paths.get(&path) {
        acquire(state, txn, handle)?;
        return Ok(handle);
    }
    let bytes = std::fs::read(&path).map_err(memory)?;
    let (pe, mut image, table) = prepare(&bytes)?;
    let imports = pe.imports(&bytes).map_err(bad)?;
    let (base, size) = state.modules.allocate(&pe, mem)?;
    relocate(&pe, &mut image, base)?;
    let tls = if let Some(spec) = static_tls(&pe, &image, base)? {
        let index = (0..TLS_SLOTS)
            .find(|&index| {
                !state
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
    state.modules.paths.insert(path.clone(), base);
    state.modules.images.insert(
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
        let dependency = link(&dll.dll, path.parent(), mem, api, state, txn, depth + 1)?;
        attach_dependency(base, dependency, state, txn);
        for function in dll.functions {
            if function.iat_rva as u64 + 4 > pe.size_of_image as u64 {
                return Err(bad("IAT out of image"));
            }
            let symbol = match function.name {
                Some(name) => Symbol::Name(name),
                None => Symbol::Ordinal(function.ordinal.unwrap()),
            };
            let target = resolve(
                dependency,
                symbol,
                Some(base),
                mem,
                api,
                state,
                txn,
                &mut HashSet::new(),
            )?;
            mem.write_u32(base + function.iat_rva, target)
                .map_err(memory)?;
        }
    }
    mem.protect_image(base, &pe)
        .map_err(|e| Failure::Expected(e, "image protection failed".into()))?;
    state.modules.images.get_mut(&base).unwrap().phase = Phase::Linked;
    Ok(base)
}
fn acquire(state: &mut ApiState, txn: &mut Transaction, handle: u32) -> LoadResult<()> {
    if let Some(module) = state.modules.images.get_mut(&handle) {
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
#[allow(clippy::too_many_arguments)]
fn resolve(
    handle: u32,
    symbol: Symbol,
    owner: Option<u32>,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
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
    if let Some(dll) = state.handle_to_dll.get(&handle) {
        if let Symbol::Name(name) = &symbol {
            if let Some(addr) = api
                .data_export(mem, dll, name, state.cmdline_a, state.cmdline_w)
                .map_err(Failure::Fatal)?
            {
                return Ok(addr);
            }
        }
        return match &symbol {
            Symbol::Name(name)
                if crate::api::native_export(dll, name) || api.lookup(dll, name).is_some() =>
            {
                Ok(api.register(&dll.clone(), Some(name), None))
            }
            Symbol::Ordinal(ordinal) if crate::win32_abi::ordinal_name(dll, *ordinal).is_some() => {
                Ok(api.register(&dll.clone(), None, Some(*ordinal)))
            }
            Symbol::Ordinal(ordinal) => api
                .entries
                .iter()
                .position(|e| &e.dll == dll && e.ordinal == Some(*ordinal))
                .map(|i| crate::api::STUB_BASE + i as u32 * crate::api::STUB_STRIDE)
                .ok_or_else(|| Failure::Expected(127, format!("native export {key} unavailable"))),
            _ => Err(Failure::Expected(
                127,
                format!("native export {key} unavailable"),
            )),
        };
    }
    let module = state
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
            let next = link(dll, parent.as_deref(), mem, api, state, txn, seen.len())?;
            if let Some(owner) = owner {
                attach_dependency(owner, next, state, txn);
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
            resolve(next, symbol, owner, mem, api, state, txn, seen)
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn invoke_tls(
    handle: u32,
    reason: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> LoadResult<()> {
    let callbacks = state
        .modules
        .images
        .get(&handle)
        .and_then(|m| m.tls.as_ref())
        .map(|t| t.callbacks.clone())
        .unwrap_or_default();
    for function in callbacks {
        if !state.modules.images.contains_key(&handle) {
            break;
        }
        log.note(&format!(
            "TLS callback module={handle:#x} entry={function:#x} reason={reason}"
        ));
        match cpu
            .call_guest(
                mem,
                api,
                state,
                log,
                sched,
                function,
                &[handle, reason, 0],
                max_instr,
            )
            .map_err(Failure::Fatal)?
        {
            GuestCallOutcome::Returned(_) => {} // TLS callbacks return void.
            outcome => return Err(Failure::Control(outcome)),
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn notification(
    handle: u32,
    reason: u32,
    reserved: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> LoadResult<()> {
    invoke_tls(handle, reason, cpu, mem, api, state, log, sched, max_instr)?;
    let entry = state.modules.images.get(&handle).map_or(0, |m| m.entry);
    if entry != 0 {
        match cpu
            .call_guest(
                mem,
                api,
                state,
                log,
                sched,
                entry,
                &[handle, reason, reserved],
                max_instr,
            )
            .map_err(Failure::Fatal)?
        {
            GuestCallOutcome::Returned(_) => {}
            outcome => return Err(Failure::Control(outcome)),
        }
    }
    Ok(())
}
fn notification_result(result: LoadResult<()>) -> Result<GuestCallOutcome, String> {
    match result {
        Ok(()) => Ok(GuestCallOutcome::Returned(1)),
        Err(Failure::Control(outcome)) => Ok(outcome),
        Err(Failure::Fatal(message) | Failure::Expected(_, message)) => Err(message),
    }
}
/// Execute notifications in the owning thread, after TLS storage exists and
/// before its entry, or before releasing that storage on orderly thread exit.
#[allow(clippy::too_many_arguments)]
pub fn notify_thread(
    main: bool,
    attach: bool,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<GuestCallOutcome, String> {
    if state.modules.process_detaching {
        return Ok(GuestCallOutcome::Returned(1));
    }
    let reason = if attach {
        if main {
            1
        } else {
            2
        }
    } else {
        3
    };
    let result = (|| -> LoadResult<()> {
        if !main || !attach {
            let mut order = state.modules.init_order.clone();
            if !attach {
                order.reverse();
            }
            for handle in order {
                if handle == state.exe_base {
                    continue;
                }
                let eligible = state
                    .modules
                    .images
                    .get(&handle)
                    .is_some_and(|m| m.phase == Phase::Ready && !m.thread_calls_disabled);
                if eligible {
                    notification(
                        handle, reason, 0, cpu, mem, api, state, log, sched, max_instr,
                    )?;
                }
            }
        }
        invoke_tls(
            state.exe_base,
            reason,
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            max_instr,
        )
    })();
    notification_result(result)
}
#[allow(clippy::too_many_arguments)]
pub fn notify_process_exit(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<GuestCallOutcome, String> {
    if state.modules.process_detaching {
        return Ok(GuestCallOutcome::Returned(1));
    }
    state.modules.process_detaching = true;
    let order = state.modules.init_order.clone();
    let result = (|| -> LoadResult<()> {
        for handle in order.into_iter().rev() {
            if state
                .modules
                .images
                .get(&handle)
                .is_some_and(|m| m.phase == Phase::Ready)
            {
                notification(handle, 0, 1, cpu, mem, api, state, log, sched, max_instr)?;
            }
        }
        Ok(())
    })();
    notification_result(result)
}

#[allow(clippy::too_many_arguments)]
fn initialize(
    handle: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
    txn: &mut Transaction,
) -> LoadResult<()> {
    let Some(module) = state.modules.images.get_mut(&handle) else {
        return Ok(());
    };
    if matches!(module.phase, Phase::Initializing | Phase::Ready) {
        return Ok(());
    }
    if module.phase != Phase::Linked {
        return Err(bad("module graph not fully linked"));
    }
    module.phase = Phase::Initializing;
    let dependencies = module.dependencies.clone();
    let entry = module.entry;
    let path = module.path.clone();
    // Optional install-time sidecar. Its source hash and each instruction's
    // loaded bytes must match; stale/unsupported IR never prevents loading.
    let sidecar = path.with_file_name(format!(
        "{}.wir",
        path.file_name().unwrap().to_string_lossy()
    ));
    if cpu.uses_ir() && sidecar.is_file() {
        let result = (|| -> Result<usize, String> {
            let bytes = std::fs::read(&sidecar).map_err(|e| e.to_string())?;
            let source = std::fs::read(&path).map_err(|e| e.to_string())?;
            let image = crate::ir::Image::parse(&bytes, &source)?.at_base(handle)?;
            cpu.add_module_ir(image, mem)
        })();
        log.note(&format!("module IR {}: {result:?}", path.display()));
    }
    for dependency in dependencies {
        initialize(dependency, cpu, mem, api, state, log, sched, max_instr, txn)?;
    }
    invoke_tls(handle, 1, cpu, mem, api, state, log, sched, max_instr)?;
    if entry != 0 {
        log.note(&format!(
            "DllMain attach module={handle:#x} entry={entry:#x}"
        ));
        match cpu
            .call_guest(
                mem,
                api,
                state,
                log,
                sched,
                entry,
                &[handle, 1, 0],
                max_instr,
            )
            .map_err(Failure::Fatal)?
        {
            GuestCallOutcome::Returned(0) => {
                // A failed PROCESS_ATTACH is followed by PROCESS_DETACH while
                // its dependencies and static TLS are still mapped.
                txn.initialized.push(handle);
                return Err(Failure::Expected(1114, "DllMain returned FALSE".into()));
            }
            GuestCallOutcome::Returned(_) => {}
            outcome => return Err(Failure::Control(outcome)),
        }
    }
    state.modules.images.get_mut(&handle).unwrap().phase = Phase::Ready;
    txn.initialized.push(handle);
    state.modules.init_order.push(handle);
    Ok(())
}
fn rollback(
    state: &mut ApiState,
    mem: &mut GuestMem,
    cpu: &mut Cpu,
    txn: &Transaction,
) -> Result<(), String> {
    for &(owner, dependency) in txn.edges.iter().rev() {
        if let Some(module) = state.modules.images.get_mut(&owner) {
            module.dependencies.retain(|&h| h != dependency);
        }
    }
    for &handle in txn.acquired.iter().rev() {
        if let Some(module) = state.modules.images.get_mut(&handle) {
            module.refs -= 1;
        }
    }
    for &handle in txn.created.iter().rev() {
        if let Some(mut module) = state.modules.images.remove(&handle) {
            state.modules.release_module_tls(
                &mut module,
                mem,
                &mut state.heap,
                state.process_heap,
            )?;
            state.modules.paths.remove(&module.path);
            state.modules.init_order.retain(|&h| h != handle);
            mem.decommit_image(handle, module.size)
                .map_err(|e| e.to_string())?;
            cpu.remove_module_ir(handle, module.size);
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn unload(
    handle: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> LoadResult<()> {
    if state.handle_to_dll.contains_key(&handle) {
        return Ok(());
    }
    let module = state
        .modules
        .images
        .get_mut(&handle)
        .ok_or_else(|| Failure::Expected(6, "invalid module handle".into()))?;
    if module.pinned {
        return Ok(());
    }
    if module.phase != Phase::Ready {
        return Err(Failure::Expected(
            6,
            "module is not ready for unload".into(),
        ));
    }
    if module.refs > 1 {
        module.refs -= 1;
        return Ok(());
    }
    module.phase = Phase::Unloading;
    let entry = module.entry;
    invoke_tls(handle, 0, cpu, mem, api, state, log, sched, max_instr)?;
    if entry != 0 {
        log.note(&format!(
            "DllMain detach module={handle:#x} entry={entry:#x}"
        ));
        if let outcome @ (GuestCallOutcome::Transferred | GuestCallOutcome::Exited(_)) = cpu
            .call_guest(
                mem,
                api,
                state,
                log,
                sched,
                entry,
                &[handle, 0, 0],
                max_instr,
            )
            .map_err(Failure::Fatal)?
        {
            return Err(Failure::Control(outcome));
        }
    }
    let mut module = state.modules.images.remove(&handle).unwrap();
    state
        .modules
        .release_module_tls(&mut module, mem, &mut state.heap, state.process_heap)
        .map_err(Failure::Fatal)?;
    state.modules.paths.remove(&module.path);
    state.modules.init_order.retain(|&h| h != handle);
    mem.decommit_image(handle, module.size).map_err(memory)?;
    cpu.remove_module_ir(handle, module.size);
    for dependency in module.dependencies {
        unload(dependency, cpu, mem, api, state, log, sched, max_instr)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch(
    name: &str,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    let mut txn = Transaction::default();
    let mut note = String::new();
    let result = (|| -> LoadResult<u32> {
        match name {
            "LoadLibraryA" | "LoadLibraryW" | "LoadLibraryExA" | "LoadLibraryExW" => {
                if name.starts_with("LoadLibraryEx") && (a[1] != 0 || a[2] & !8 != 0) {
                    return Err(Failure::Expected(
                        87,
                        "unsupported LoadLibraryEx flags/file handle".into(),
                    ));
                }
                let path = crate::api::read_str(mem, a[0], name.ends_with('W'));
                let handle =
                    link(&path, None, mem, api, state, &mut txn, 0).map_err(|e| match e {
                        Failure::Expected(code, reason) => {
                            Failure::Expected(code, format!("{path:?}: {reason}"))
                        }
                        other => other,
                    })?;
                for thread in &sched.threads {
                    if !matches!(thread.state, crate::thread::ThreadState::Exited(_)) {
                        state
                            .modules
                            .setup_tls_thread(mem, &mut state.heap, state.process_heap, thread.teb)
                            .map_err(Failure::Fatal)?;
                    }
                }
                initialize(
                    handle, cpu, mem, api, state, log, sched, max_instr, &mut txn,
                )?;
                note = format!("loaded {path:?} at {handle:#x}");
                Ok(handle)
            }
            "DisableThreadLibraryCalls" => {
                if state.handle_to_dll.contains_key(&a[0]) {
                    return Ok(1);
                }
                let module = state
                    .modules
                    .images
                    .get_mut(&a[0])
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
            "FreeLibrary" => {
                unload(a[0], cpu, mem, api, state, log, sched, max_instr)?;
                Ok(1)
            }
            "GetProcAddress" => {
                let symbol = if a[1] >> 16 == 0 {
                    Symbol::Ordinal(a[1] as u16)
                } else {
                    Symbol::Name(mem.read_cstr(a[1], 1024).map_err(memory)?)
                };
                note = format!("module={:#x} export={symbol:?}", a[0]);
                let owner = state.modules.images.contains_key(&a[0]).then_some(a[0]);
                let value = resolve(
                    a[0],
                    symbol,
                    owner,
                    mem,
                    api,
                    state,
                    &mut txn,
                    &mut HashSet::new(),
                )?;
                for handle in txn.created.clone() {
                    for thread in &sched.threads {
                        if !matches!(thread.state, crate::thread::ThreadState::Exited(_)) {
                            state
                                .modules
                                .setup_tls_thread(
                                    mem,
                                    &mut state.heap,
                                    state.process_heap,
                                    thread.teb,
                                )
                                .map_err(Failure::Fatal)?;
                        }
                    }
                    initialize(
                        handle, cpu, mem, api, state, log, sched, max_instr, &mut txn,
                    )?;
                }
                Ok(value)
            }
            "GetModuleHandleA" | "GetModuleHandleW" | "GetModuleHandleExA"
            | "GetModuleHandleExW" => {
                let extended = name.starts_with("GetModuleHandleEx");
                let flags = if extended { a[0] } else { 2 };
                if flags & !7 != 0 || flags & 3 == 3 {
                    return Err(Failure::Expected(87, "invalid module handle flags".into()));
                }
                let pointer = if extended { a[1] } else { a[0] };
                let handle = if flags & 4 != 0 {
                    state.modules.address(pointer).or_else(|| {
                        api.stub_index(pointer)
                            .and_then(|i| state.module_handles.get(&api.entries[i].dll).copied())
                    })
                } else if pointer == 0 {
                    Some(state.exe_base)
                } else {
                    let path = crate::api::read_str(mem, pointer, name.ends_with('W'));
                    state.modules.handle(&path, state.fs.as_ref()).or_else(|| {
                        state
                            .module_handles
                            .get(&basename(&library_name(&path)))
                            .copied()
                    })
                }
                .ok_or_else(|| Failure::Expected(126, "module is not loaded".into()))?;
                if extended {
                    mem.ptr(a[2], 4, true).map_err(memory)?;
                    if flags & 2 == 0 {
                        acquire(state, &mut txn, handle)?;
                    }
                    if flags & 1 != 0 {
                        if let Some(module) = state.modules.images.get_mut(&handle) {
                            module.pinned = true;
                        }
                    }
                    mem.write_u32(a[2], handle).map_err(memory)?;
                    Ok(1)
                } else {
                    Ok(handle)
                }
            }
            "GetModuleFileNameA" | "GetModuleFileNameW" => {
                let path = if a[0] == 0 || a[0] == state.exe_base {
                    state.exe_path.clone()
                } else if let Some(module) = state.modules.images.get(&a[0]) {
                    module.guest_path.clone()
                } else if let Some(path) = state.handle_to_dll.get(&a[0]) {
                    format!("C:\\Windows\\System32\\{path}")
                } else {
                    return Err(Failure::Expected(6, "invalid module handle".into()));
                };
                let wide = name.ends_with('W');
                let length = if wide {
                    path.encode_utf16().count()
                } else {
                    crate::codepage::encode(&path, 932)
                        .map_err(|e| {
                            Failure::Expected(e, "module filename encoding failed".into())
                        })?
                        .0
                        .len()
                } as u32;
                if a[2] == 0 {
                    state.last_error = 122;
                    return Ok(0);
                }
                let mut bytes = if wide {
                    path.encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&path, 932)
                        .map_err(|e| {
                            Failure::Expected(e, "module filename encoding failed".into())
                        })?
                        .0
                };
                let width = if wide { 2 } else { 1 };
                bytes.truncate(a[2].saturating_sub(1) as usize * width);
                bytes.extend(std::iter::repeat_n(0, width));
                mem.write_bytes(a[1], &bytes).map_err(memory)?;
                if length >= a[2] {
                    state.last_error = 122;
                    Ok(a[2])
                } else {
                    Ok(length)
                }
            }
            _ => unreachable!(),
        }
    })();
    match result {
        Ok(value) => Ok((GuestCallOutcome::Returned(value), note)),
        Err(Failure::Expected(code, message)) => {
            // Undo successful dependency callbacks in reverse init order.
            for &handle in txn.initialized.iter().rev() {
                let entry = state.modules.images[&handle].entry;
                if let Err(failure) =
                    invoke_tls(handle, 0, cpu, mem, api, state, log, sched, max_instr)
                {
                    abandon(state, &txn);
                    return match failure {
                        Failure::Control(outcome) => {
                            Ok((outcome, "TLS rollback transferred control".into()))
                        }
                        Failure::Fatal(message) | Failure::Expected(_, message) => Err(message),
                    };
                }
                if entry != 0 {
                    match cpu.call_guest(
                        mem,
                        api,
                        state,
                        log,
                        sched,
                        entry,
                        &[handle, 0, 0],
                        max_instr,
                    )? {
                        GuestCallOutcome::Returned(_) => {}
                        outcome => {
                            abandon(state, &txn);
                            return Ok((
                                outcome,
                                "DLL rollback callback transferred control".into(),
                            ));
                        }
                    }
                }
            }
            rollback(state, mem, cpu, &txn)?;
            state.last_error = code;
            Ok((GuestCallOutcome::Returned(0), message))
        }
        Err(Failure::Fatal(message)) => {
            rollback(state, mem, cpu, &txn)?;
            Err(message)
        }
        // Guest exception/exit already chose a new architectural context.
        // Preserve mapped code while it may still be executing there.
        Err(Failure::Control(outcome)) => {
            abandon(state, &txn);
            Ok((outcome, "DLL callback transferred control".into()))
        }
    }
}
