//! Behavioral tests for the Win32 subset, independent of a proprietary EXE.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use wire::api::{self, ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::Cpu;
use wire::fs::GuestFs;
use wire::loader::{STACK_BASE, STACK_SIZE, TEB_ADDR};
use wire::mem::GuestMem;
use wire::thread::Sched;
const DATA: u32 = 0x20000;
const CODE: u32 = 0x10000;
const RET: u32 = CODE + 0x800;

struct Vm {
    mem: GuestMem,
    cpu: Cpu,
    api: ApiTable,
    state: ApiState,
    log: ApiLogger,
    sched: Sched,
    dir: PathBuf,
}
impl Vm {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wire-win32-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let mut mem = GuestMem::new().unwrap();
        for (p, n) in [
            (CODE, 0x10000),
            (DATA, 0x10000),
            (STACK_BASE, STACK_SIZE),
            (TEB_ADDR, 0x1000),
        ] {
            mem.commit(p, n).unwrap();
        }
        mem.write_u32(TEB_ADDR, u32::MAX).unwrap();
        mem.write_u32(TEB_ADDR + 4, STACK_BASE + STACK_SIZE)
            .unwrap();
        mem.write_u32(TEB_ADDR + 8, STACK_BASE).unwrap();
        let cpu = Cpu::new(CODE, STACK_BASE + STACK_SIZE - 256, TEB_ADDR);
        let api = ApiTable::new();
        let mut state = ApiState::new(0x400000, "C:\\test.exe".into(), 0, 0);
        state.fs = Some(GuestFs::new(&dir).unwrap());
        let log = ApiLogger::new(&dir.join("api.log")).unwrap();
        let mut sched = Sched::new();
        let tid = sched.alloc_tid();
        let handle = state.alloc_handle(HandleObj::Thread { tid });
        sched.add_main(tid, handle, TEB_ADDR);
        Self {
            mem,
            cpu,
            api,
            state,
            log,
            sched,
            dir,
        }
    }
    fn call(&mut self, dll: &str, name: &str, args: &[u32]) -> u32 {
        self.try_call(dll, name, args).unwrap()
    }
    fn try_call(&mut self, dll: &str, name: &str, args: &[u32]) -> Result<u32, String> {
        let addr = self.api.register(dll, Some(name), None);
        self.cpu.eip = addr;
        self.cpu.set_esp(STACK_BASE + STACK_SIZE - 256);
        let sp = self.cpu.esp();
        self.mem.write_u32(sp, RET).unwrap();
        for i in 0..12 {
            self.mem
                .write_u32(sp + 4 + i * 4, args.get(i as usize).copied().unwrap_or(0))
                .unwrap();
        }
        let idx = self.api.stub_index(addr).unwrap();
        api::dispatch(
            idx,
            &mut self.cpu,
            &mut self.mem,
            &mut self.api,
            &mut self.state,
            &mut self.log,
            &mut self.sched,
            1_000_000,
        )?;
        Ok(self.cpu.regs[0])
    }
    fn k(&mut self, name: &str, args: &[u32]) -> u32 {
        self.call("kernel32.dll", name, args)
    }
    fn wide(&self, p: u32, text: &str) {
        for (i, c) in text.encode_utf16().chain([0]).enumerate() {
            self.mem.write_u16(p + i as u32 * 2, c).unwrap();
        }
    }
}
impl Drop for Vm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Independently authored PE32: one section, a real stdcall DllMain, code/data
/// exports, a forwarder and an ordinal hole. Absolute image pointers require
/// HIGHLOW relocation; host test result addresses are deliberately external.
fn test_dll(import: &str, attach_ok: bool, value: u32) -> Vec<u8> {
    fn w16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn w32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put(bytes: &mut [u8], rva: usize, data: &[u8]) {
        bytes[0x200 + rva - 0x1000..0x200 + rva - 0x1000 + data.len()].copy_from_slice(data);
    }
    let mut bytes = vec![0; 0x2200];
    bytes[..2].copy_from_slice(b"MZ");
    w32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    w16(&mut bytes, 0x84, 0x14c);
    w16(&mut bytes, 0x86, 1);
    w16(&mut bytes, 0x94, 0xe0);
    w16(&mut bytes, 0x96, 0x2102);
    let opt = 0x98;
    w16(&mut bytes, opt, 0x10b);
    w32(&mut bytes, opt + 16, 0x1000);
    w32(&mut bytes, opt + 28, 0x10000000);
    w32(&mut bytes, opt + 32, 0x1000);
    w32(&mut bytes, opt + 36, 0x200);
    w32(&mut bytes, opt + 56, 0x4000);
    w32(&mut bytes, opt + 60, 0x200);
    w32(&mut bytes, opt + 92, 16);
    w32(&mut bytes, opt + 96, 0x1500);
    w32(&mut bytes, opt + 100, 0x200);
    w32(&mut bytes, opt + 104, 0x1300);
    w32(&mut bytes, opt + 108, 40);
    w32(&mut bytes, opt + 136, 0x1700);
    bytes[0x178..0x180].copy_from_slice(b".test\0\0\0");
    w32(&mut bytes, 0x180, 0x3000);
    w32(&mut bytes, 0x184, 0x1000);
    w32(&mut bytes, 0x188, 0x2000);
    w32(&mut bytes, 0x18c, 0x200);
    w32(&mut bytes, 0x19c, 0xe0000020);
    let mut body = vec![
        0x53, 0x8b, 0x5c, 0x24, 8, 0x8b, 0x44, 0x24, 12, 0x85, 0xc0, 0x74, 0,
    ];
    body.extend([0xff, 5]);
    body.extend((DATA + 0x700).to_le_bytes());
    body.extend([0x89, 0x1d]);
    body.extend((DATA + 0x708).to_le_bytes());
    body.extend([0xff, 0x15]);
    let iat_reloc = body.len() as u16;
    body.extend(0x10001400u32.to_le_bytes());
    body.push(0xa3);
    body.extend((DATA + 0x70c).to_le_bytes());
    body.push(0xb8);
    body.extend(u32::from(attach_ok).to_le_bytes());
    body.extend([0x5b, 0xc2, 12, 0]);
    body[12] = (body.len() - 13) as u8;
    body.extend([0xff, 5]);
    body.extend((DATA + 0x704).to_le_bytes());
    body.extend([0xb8, 1, 0, 0, 0, 0x5b, 0xc2, 12, 0]);
    put(&mut bytes, 0x1000, &body);
    put(&mut bytes, 0x1080, &[0xa1, 0, 0x12, 0, 0x10, 0xc3]);
    put(&mut bytes, 0x1200, &value.to_le_bytes());
    put(&mut bytes, 0x1210, &0x10001200u32.to_le_bytes());
    let mut imports = [0; 40];
    w32(&mut imports, 0, 0x1380);
    w32(&mut imports, 12, 0x1340);
    w32(&mut imports, 16, 0x1400);
    put(&mut bytes, 0x1300, &imports);
    let (dll, function) = import.rsplit_once('!').unwrap();
    put(&mut bytes, 0x1340, format!("{dll}\0").as_bytes());
    put(&mut bytes, 0x1380, &0x13a0u32.to_le_bytes());
    put(&mut bytes, 0x13a2, format!("{function}\0").as_bytes());
    put(&mut bytes, 0x1400, &0x13a0u32.to_le_bytes());
    let mut table = [0; 40];
    for (offset, val) in [
        (16, 7),
        (20, 5),
        (24, 4),
        (28, 0x1530),
        (32, 0x1548),
        (36, 0x1560),
    ] {
        w32(&mut table, offset, val);
    }
    put(&mut bytes, 0x1500, &table);
    for (index, rva) in [0x1080u32, 0x1200, 0x1210, 0x15e0, 0]
        .into_iter()
        .enumerate()
    {
        put(&mut bytes, 0x1530 + index * 4, &rva.to_le_bytes());
    }
    let mut at = 0x1570;
    for (index, (name, ordinal)) in [("Alias", 3u16), ("Data", 1), ("GetValue", 0), ("Ptr", 2)]
        .into_iter()
        .enumerate()
    {
        put(&mut bytes, 0x1548 + index * 4, &(at as u32).to_le_bytes());
        put(&mut bytes, 0x1560 + index * 2, &ordinal.to_le_bytes());
        let text = format!("{name}\0");
        put(&mut bytes, at, text.as_bytes());
        at += text.len();
    }
    put(&mut bytes, 0x15e0, b"kernel32.GetCurrentProcessId\0");
    let mut relocations = vec![0; 8];
    w32(&mut relocations, 0, 0x1000);
    for offset in [iat_reloc, 0x81, 0x210, 0] {
        relocations.extend((if offset == 0 { 0 } else { 0x3000 | offset }).to_le_bytes());
    }
    let reloc_size = relocations.len() as u32;
    w32(&mut relocations, 4, reloc_size);
    put(&mut bytes, 0x1700, &relocations);
    w32(&mut bytes, opt + 140, reloc_size);
    bytes
}

fn dll_export(v: &mut Vm, module: u32, name: &str) -> u32 {
    v.mem
        .write_bytes(DATA + 0x100, format!("{name}\0").as_bytes())
        .unwrap();
    v.k("GetProcAddress", &[module, DATA + 0x100])
}

fn test_tls_dll(attach_ok: bool) -> Vec<u8> {
    let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", attach_ok, 0);
    let put = |bytes: &mut [u8], rva: usize, data: &[u8]| {
        let at = 0x200 + rva - 0x1000;
        bytes[at..at + data.len()].copy_from_slice(data);
    };
    bytes[0x140..0x144].copy_from_slice(&0x1800u32.to_le_bytes());
    bytes[0x144..0x148].copy_from_slice(&24u32.to_le_bytes());
    for (i, value) in [
        0x10001900u32,
        0x10001904,
        0x10001920,
        0x10001930,
        12,
        0x500000,
    ]
    .into_iter()
    .enumerate()
    {
        put(&mut bytes, 0x1800 + i * 4, &value.to_le_bytes());
    }
    put(&mut bytes, 0x1900, &37u32.to_le_bytes());
    let mut access = vec![0x64, 0x8b, 0x0d, 0x2c, 0, 0, 0, 0xa1];
    access.extend(0x10001920u32.to_le_bytes());
    access.extend([0x8b, 0x04, 0x81, 0x8b, 0x10]);
    let mut entry = access.clone();
    entry.extend([0x89, 0x15]);
    entry.extend((DATA + 0x720).to_le_bytes());
    entry.push(0xb8);
    entry.extend(u32::from(attach_ok).to_le_bytes());
    entry.extend([0xc2, 12, 0]);
    put(&mut bytes, 0x1000, &entry);
    access.extend([0xff, 0x00, 0x89, 0xd0, 0xc3]); // return previous TLS value, increment it
    put(&mut bytes, 0x1080, &access);
    let mut reloc = Vec::from(0x1000u32.to_le_bytes());
    reloc.extend(20u32.to_le_bytes());
    for offset in [0x3008u16, 0x3088, 0x3800, 0x3804, 0x3808, 0x380c] {
        reloc.extend(offset.to_le_bytes());
    }
    put(&mut bytes, 0x1700, &reloc);
    bytes[0x124..0x128].copy_from_slice(&(reloc.len() as u32).to_le_bytes());
    bytes
}

#[test]
fn dll_static_tls_is_ready_for_dllmain_and_isolated_for_existing_and_new_threads() {
    let mut v = Vm::new();
    let existing = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let existing_teb = v.sched.threads[v.sched.thread_index_by_handle(existing).unwrap()].teb;
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(true)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(module, 0x50000000);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37); // read by actual guest DllMain
    let function = dll_export(&mut v, module, "GetValue");
    let call = |v: &mut Vm| {
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000,
            )
            .unwrap()
    };
    assert!(matches!(
        call(&mut v),
        wire::cpu::GuestCallOutcome::Returned(37)
    ));
    assert!(matches!(
        call(&mut v),
        wire::cpu::GuestCallOutcome::Returned(38)
    ));
    let new = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let new_teb = v.sched.threads[v.sched.thread_index_by_handle(new).unwrap()].teb;
    let index = v.mem.read_u32(module + 0x1920).unwrap();
    let main_vector = v.mem.read_u32(TEB_ADDR + 0x2c).unwrap();
    let main_block = v.mem.read_u32(main_vector + index * 4).unwrap();
    assert_eq!(v.mem.read_u32(main_block).unwrap(), 39);
    for teb in [existing_teb, new_teb] {
        let vector = v.mem.read_u32(teb + 0x2c).unwrap();
        let block = v.mem.read_u32(vector + index * 4).unwrap();
        assert_ne!(block, main_block);
        assert_eq!(v.mem.read_u32(block).unwrap(), 37);
        let mut zero = [1; 12];
        v.mem.read_bytes(block + 4, &mut zero).unwrap();
        assert_eq!(zero, [0; 12]);
        let saved_teb = v.cpu.fs_base;
        v.cpu.fs_base = teb;
        assert!(matches!(
            call(&mut v),
            wire::cpu::GuestCallOutcome::Returned(37)
        ));
        v.cpu.fs_base = saved_teb;
    }
    assert_eq!(v.k("FreeLibrary", &[module]), 1);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, main_block]),
        u32::MAX
    );
    for teb in [TEB_ADDR, existing_teb, new_teb] {
        let vector = v.mem.read_u32(teb + 0x2c).unwrap();
        assert_eq!(v.mem.read_u32(vector + index * 4).unwrap(), 0);
    }
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), module);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37); // fresh template after reload
}

#[test]
fn dll_tls_failure_rolls_back_and_unsupported_callbacks_are_rejected() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(false)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1114);
    assert!(!v.mem.is_committed(0x50000000));
    let vector = v.mem.read_u32(TEB_ADDR + 0x2c).unwrap();
    assert_eq!(v.mem.read_u32(vector).unwrap(), 0);
    let mut callback = test_tls_dll(true);
    callback[0xb30..0xb34].copy_from_slice(&0x10001000u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), callback).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(0x50000000));
    let mut bad = test_tls_dll(true);
    bad[0xa08..0xa0c].copy_from_slice(&0x10004000u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), bad).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(0x50000000));
}

#[test]
fn dll_mapping_relocation_exports_initialization_and_reference_counts() {
    let mut v = Vm::new();
    let heap = v.state.process_heap;
    let allocation = v.k("HeapAlloc", &[heap, 0, 64]);
    v.mem.write_u32(allocation, 0xdead1234).unwrap();
    std::fs::write(
        v.dir.join("demo.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0x12345678),
    )
    .unwrap();
    v.wide(DATA, "C:\\demo.dll");
    let handle = v.k("LoadLibraryW", &[DATA]);
    assert_eq!(handle, 0x50000000);
    assert_eq!(v.mem.read_u32(handle + 0x1210).unwrap(), handle + 0x1200);
    assert_eq!(v.mem.read_u32(allocation).unwrap(), 0xdead1234);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x708).unwrap(), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 4242);
    let function = dll_export(&mut v, handle, "GetValue");
    assert_eq!(function, handle + 0x1080);
    assert_eq!(v.k("GetProcAddress", &[handle, 7]), function);
    assert_eq!(dll_export(&mut v, handle, "Data"), handle + 0x1200);
    assert_eq!(dll_export(&mut v, handle, "getvalue"), 0);
    assert_eq!(dll_export(&mut v, handle, "GetPluginInfo"), 0);
    assert_eq!(v.k("GetLastError", &[]), 127);
    assert_eq!(v.k("GetProcAddress", &[handle, 11]), 0);
    assert_eq!(
        dll_export(&mut v, handle, "Alias"),
        v.api.lookup("kernel32.dll", "GetCurrentProcessId").unwrap()
    );
    assert!(matches!(
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000
            )
            .unwrap(),
        wire::cpu::GuestCallOutcome::Returned(0x12345678)
    ));
    v.mem.write_bytes(DATA, b"DEMO.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 1);
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), handle);
    assert_eq!(v.k("GetModuleHandleExA", &[6, function, DATA + 0x300]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), handle);
    assert_eq!(v.k("GetModuleFileNameW", &[handle, DATA + 0x400, 260]), 11);
    assert_eq!(v.mem.read_wstr(DATA + 0x400, 260).unwrap(), "C:\\demo.dll");
    assert_eq!(v.k("GetModuleFileNameW", &[handle, DATA + 0x400, 4]), 4);
    assert_eq!(v.mem.read_wstr(DATA + 0x400, 4).unwrap(), "C:\\");
    assert_eq!(v.k("FreeLibrary", &[handle]), 1);
    assert!(v.mem.is_committed(handle));
    assert_eq!(v.k("FreeLibrary", &[handle]), 1);
    assert!(!v.mem.is_committed(handle));
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 1);
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
    assert_eq!(v.k("GetProcAddress", &[handle, 7]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(v.k("LoadLibraryA", &[DATA]), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 2);
    assert_eq!(v.mem.read_u32(handle + 0x1200).unwrap(), 0x12345678);
}

#[test]
fn dll_dependencies_initialize_before_parent_and_failure_rolls_back() {
    let mut v = Vm::new();
    std::fs::write(
        v.dir.join("child.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0xcafe),
    )
    .unwrap();
    std::fs::write(
        v.dir.join("parent.dll"),
        test_dll("child.dll!GetValue", true, 0xbeef),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"parent.dll\0").unwrap();
    let parent = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(parent, 0x50000000);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 0xcafe);
    assert_eq!(v.mem.read_u32(parent + 0x1400).unwrap(), 0x50011080);
    assert_eq!(v.k("FreeLibrary", &[parent]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 2);
    assert!(!v.mem.is_committed(0x50010000));

    std::fs::write(
        v.dir.join("bad.dll"),
        test_dll("child.dll!GetValue", false, 0xbeef),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"bad.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1114);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 4);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 4); // failed parent and successful child detached
    assert!(!v.mem.is_committed(0x50000000));
    assert!(!v.mem.is_committed(0x50010000));
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
}

#[test]
fn dll_guest_forwarders_deduplicate_dependencies_and_failed_load_preserves_existing_modules() {
    let mut v = Vm::new();
    let child = test_dll("kernel32.dll!GetCurrentProcessId", true, 73);
    std::fs::write(v.dir.join("child.dll"), &child).unwrap();
    v.mem.write_bytes(DATA, b"child.dll\0").unwrap();
    let child_handle = v.k("LoadLibraryA", &[DATA]);
    std::fs::write(
        v.dir.join("bad.dll"),
        test_dll("child.dll!GetValue", false, 0),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"bad.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 1); // failed parent detached, existing child kept
    assert!(v.mem.is_committed(child_handle));
    assert_eq!(v.k("FreeLibrary", &[child_handle]), 1); // failed transaction released its ref
    assert!(!v.mem.is_committed(child_handle));

    let alias = |forward: &str| {
        let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 81);
        bytes[0x7e0..0x800].fill(0);
        bytes[0x7e0..0x7e0 + forward.len()].copy_from_slice(forward.as_bytes());
        bytes
    };
    std::fs::write(v.dir.join("alias.dll"), alias("child.#7")).unwrap();
    v.mem.write_bytes(DATA, b"alias.dll\0").unwrap();
    let parent = v.k("LoadLibraryA", &[DATA]);
    for _ in 0..3 {
        assert_eq!(dll_export(&mut v, parent, "Alias"), 0x50011080);
    }
    assert_eq!(v.k("FreeLibrary", &[parent]), 1);
    assert!(!v.mem.is_committed(0x50010000));
    std::fs::write(v.dir.join("a.dll"), alias("b.Alias")).unwrap();
    std::fs::write(v.dir.join("b.dll"), alias("a.Alias")).unwrap();
    v.mem.write_bytes(DATA, b"a.dll\0").unwrap();
    let a = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(dll_export(&mut v, a, "Alias"), 0);
    assert_eq!(v.k("GetLastError", &[]), 127);
    assert!(!v.mem.is_committed(0x50010000));
    assert!(v.mem.is_committed(a));
    assert_eq!(v.k("FreeLibrary", &[a]), 1);
    assert!(!v.mem.is_committed(a));
}

#[test]
fn dll_qualified_module_names_and_native_stub_addresses_resolve_correctly() {
    let mut v = Vm::new();
    let bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 9);
    for dir in ["one", "two"] {
        std::fs::create_dir(v.dir.join(dir)).unwrap();
        std::fs::write(v.dir.join(dir).join("same.dll"), &bytes).unwrap();
    }
    v.wide(DATA, "C:\\one\\same.dll");
    let one = v.k("LoadLibraryW", &[DATA]);
    v.wide(DATA, "C:\\two\\same.dll");
    let two = v.k("LoadLibraryW", &[DATA]);
    assert_ne!(one, two);
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), two);
    v.wide(DATA, "C:\\one\\same.dll");
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), one);
    v.wide(DATA, "C:\\absent\\same.dll");
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), 0);
    let stub = v.api.lookup("kernel32.dll", "GetCurrentProcessId").unwrap();
    assert_eq!(v.k("GetModuleHandleExW", &[6, stub, DATA + 0x300]), 1);
    assert_eq!(
        v.mem.read_u32(DATA + 0x300).unwrap(),
        v.state.module_handles["kernel32.dll"]
    );
}

#[test]
fn main_image_registers_real_exports_and_honors_pe_stack_reserve() {
    let mut v = Vm::new();
    let mut exe = test_dll("kernel32.dll!GetCurrentProcessId", true, 19);
    exe[0x96..0x98].copy_from_slice(&0x102u16.to_le_bytes());
    exe[0xb4..0xb8].copy_from_slice(&0x400000u32.to_le_bytes());
    exe[0xe0..0xe4].copy_from_slice(&0x100000u32.to_le_bytes());
    let path = v.dir.join("test.exe");
    std::fs::write(&path, &exe).unwrap();
    let mut mem = GuestMem::new().unwrap();
    let mut table = ApiTable::new();
    let loaded = wire::loader::load_image(&mut mem, &mut table, &path, "test.exe").unwrap();
    let top = STACK_BASE + STACK_SIZE;
    assert_eq!(mem.read_u32(TEB_ADDR + 4).unwrap(), top);
    assert_eq!(mem.read_u32(TEB_ADDR + 8).unwrap(), top - 0x100000);
    assert!(mem.is_committed(top - 0x100000));
    assert!(!mem.is_committed(top - 0x100000 - 0x10000));
    assert!(mem.read_u8(0x17f000).is_ok()); // actual CRT stack probe previously faulted here
    v.mem = mem;
    v.api = table;
    v.mem.commit(DATA, 0x10000).unwrap();
    v.state
        .modules
        .register_main(
            &v.mem,
            loaded.image_base,
            path.clone(),
            "C:\\test.exe".into(),
        )
        .unwrap();
    assert_eq!(v.k("GetModuleHandleA", &[0]), 0x400000);
    assert_eq!(dll_export(&mut v, 0x400000, "GetValue"), 0x401080);
    assert_eq!(dll_export(&mut v, 0x400000, "Data"), 0x401200);
    assert_eq!(v.k("FreeLibrary", &[0x400000]), 1);
    assert!(v.mem.is_committed(0x400000));

    exe[0xe0..0xe4].copy_from_slice(&0x200000u32.to_le_bytes());
    std::fs::write(&path, &exe).unwrap();
    let mut mem = GuestMem::new().unwrap();
    assert!(
        wire::loader::load_image(&mut mem, &mut v.api, &path, "test.exe")
            .err()
            .unwrap()
            .contains("stack reserve")
    );
}

#[test]
fn dll_missing_imports_and_malformed_images_fail_without_fake_modules() {
    let mut v = Vm::new();
    let valid = test_dll("kernel32.dll!GetCurrentProcessId", true, 123);
    let mut cases = vec![
        (test_dll("missing.dll!Absent", true, 0), 126),
        (test_dll("kernel32.dll!MadeUpExport", true, 0), 127),
    ];
    let mut no_reloc = valid.clone();
    no_reloc[0x120..0x128].fill(0);
    cases.push((no_reloc, 193));
    let mut wrong_reloc = valid.clone();
    wrong_reloc[0x908..0x90a].copy_from_slice(&0x7001u16.to_le_bytes());
    cases.push((wrong_reloc, 193));
    let mut bad_export = valid.clone();
    bad_export[0x740..0x744].copy_from_slice(&0x4000u32.to_le_bytes());
    cases.push((bad_export, 193));
    let mut bad_section = valid.clone();
    bad_section[0x188..0x18c].copy_from_slice(&0x4000u32.to_le_bytes());
    cases.push((bad_section, 193));
    for (index, (bytes, error)) in cases.into_iter().enumerate() {
        let name = format!("bad{index}.dll");
        std::fs::write(v.dir.join(&name), bytes).unwrap();
        v.mem
            .write_bytes(DATA, format!("{name}\0").as_bytes())
            .unwrap();
        assert_eq!(v.k("LoadLibraryA", &[DATA]), 0, "{name}");
        assert_eq!(v.k("GetLastError", &[]), error, "{name}");
        assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
        assert!(!v.mem.is_committed(0x50000000));
        assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 0);
    }
    assert_eq!(v.k("LoadLibraryExA", &[DATA, 0, 2]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn heap_reuses_coalesces_and_preserves_realloc() {
    let mut v = Vm::new();
    let h = v.state.process_heap;
    let a = v.k("HeapAlloc", &[h, 0, 23]);
    let b = v.k("HeapAlloc", &[h, 0, 40]);
    v.mem.write_bytes(a, b"preserved").unwrap();
    let c = v.k("HeapReAlloc", &[h, 8, a, 100]);
    assert_ne!(c, a);
    let mut bytes = [0xff; 100];
    v.mem.read_bytes(c, &mut bytes).unwrap();
    assert_eq!(&bytes[..9], b"preserved");
    assert!(bytes[23..].iter().all(|&b| b == 0));
    assert_eq!(v.k("HeapSize", &[h, 0, c]), 100);
    assert_eq!(v.k("HeapFree", &[h, 0, b]), 1);
    assert_eq!(v.k("HeapFree", &[h, 0, c]), 1);
    let big = v.k("HeapAlloc", &[h, 8, 180]);
    assert_eq!(big, a);
    assert_eq!(v.k("HeapFree", &[h, 0, big]), 1);
    assert_eq!(v.k("HeapFree", &[h, 0, big]), 0);
    assert_eq!(v.k("HeapAlloc", &[h, 0, u32::MAX]), 0);
}

#[test]
fn heap_in_place_only_and_private_heap_ownership() {
    let mut v = Vm::new();
    let h = v.k("HeapCreate", &[0, 0, 128]);
    let a = v.k("HeapAlloc", &[h, 0, 32]);
    let b = v.k("HeapAlloc", &[h, 0, 32]);
    v.mem.write_u32(a, 0xfeed).unwrap();
    assert_eq!(v.k("HeapReAlloc", &[h, 0x10, a, 64]), 0);
    assert_eq!(v.mem.read_u32(a).unwrap(), 0xfeed);
    assert_eq!(v.k("HeapFree", &[v.state.process_heap, 0, a]), 0);
    assert_eq!(v.k("HeapFree", &[h, 0, b]), 1);
    assert_eq!(v.k("HeapReAlloc", &[h, 8, a, 64]), a);
    assert_eq!(v.k("HeapAlloc", &[h, 0, 80]), 0);
    assert_eq!(v.k("HeapDestroy", &[h]), 1);
    assert_eq!(v.k("HeapValidate", &[h, 0, 0]), 0);
}

#[test]
fn cp932_conversion_lengths_terminators_and_errors() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(DATA, &[0x93, 0xfa, 0x96, 0x7b, 0])
        .unwrap(); // 日本
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 8, DATA, u32::MAX, 0, 0]),
        3
    );
    assert_eq!(
        v.k(
            "MultiByteToWideChar",
            &[932, 8, DATA, u32::MAX, DATA + 64, 3]
        ),
        3
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 3).unwrap(), "日本");
    assert_eq!(
        v.k(
            "WideCharToMultiByte",
            &[932, 0, DATA + 64, u32::MAX, DATA + 128, 5, 0, DATA + 192]
        ),
        5
    );
    let mut bytes = [0; 5];
    v.mem.read_bytes(DATA + 128, &mut bytes).unwrap();
    assert_eq!(bytes, [0x93, 0xfa, 0x96, 0x7b, 0]);
    assert_eq!(v.mem.read_u32(DATA + 192).unwrap(), 0);
    assert_eq!(
        v.k(
            "MultiByteToWideChar",
            &[932, 8, DATA, u32::MAX, DATA + 64, 2]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 122);
    v.mem.write_bytes(DATA, &[0x81, 0]).unwrap();
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 8, DATA, 1, DATA + 64, 2]),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 1113);
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 0, DATA, 1, DATA + 64, 2]),
        1
    );
    assert_eq!(v.k("GetCPInfo", &[932, DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 6).unwrap(), 0xfce09f81);
}

#[test]
fn files_read_write_seek_eof_and_invalid_handles() {
    let mut v = Vm::new();
    v.wide(DATA, "C:\\日本.txt");
    let h = v.k("CreateFileW", &[DATA, 0xc0000000, 0, 0, 2, 0x80, 0]);
    assert_ne!(h, u32::MAX);
    v.mem.write_bytes(DATA + 256, b"abc123").unwrap();
    assert_eq!(v.k("WriteFile", &[h, DATA + 256, 6, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 6);
    assert_eq!(v.k("GetFileType", &[h]), 1);
    assert_eq!(v.k("GetFileSizeEx", &[h, DATA + 520]), 1);
    assert_eq!(v.mem.read_u64(DATA + 520).unwrap(), 6);
    assert_eq!(v.k("SetFilePointer", &[h, 3, 0, 0]), 3);
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 10, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 3);
    let mut b = [0; 3];
    v.mem.read_bytes(DATA + 300, &mut b).unwrap();
    assert_eq!(&b, b"123");
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 10, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 0);
    assert_eq!(v.k("SetFilePointerEx", &[h, 7, 1, DATA + 520, 0]), 1);
    assert_eq!(v.mem.read_u64(DATA + 520).unwrap(), 0x100000007);
    assert_eq!(v.k("SetFilePointerEx", &[h, u32::MAX, u32::MAX, 0, 0]), 0);
    assert_eq!(v.k("CloseHandle", &[h]), 1);
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 1, DATA + 512, 0]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(std::fs::read(v.dir.join("日本.txt")).unwrap(), b"abc123");
}

#[test]
fn filesystem_confines_paths_and_symlinks_and_handles_case() {
    let mut v = Vm::new();
    std::fs::create_dir(v.dir.join("Sub")).unwrap();
    std::fs::write(v.dir.join("Sub/Test.TXT"), b"ok").unwrap();
    let fs = v.state.fs.as_mut().unwrap();
    assert_eq!(
        fs.resolve("c:\\sub\\test.txt").unwrap(),
        v.dir.join("Sub/Test.TXT")
    );
    assert!(fs.resolve("../outside").is_err());
    assert!(fs.resolve("C:\\..\\outside").is_err());
    assert!(fs.resolve("\\\\server\\file").is_err());
    fs.set_cwd("Sub").unwrap();
    assert_eq!(fs.cwd(), "C:\\Sub");
    assert_eq!(
        fs.resolve("../Sub/test.txt").unwrap(),
        v.dir.join("Sub/Test.TXT")
    );
    std::os::unix::fs::symlink(std::env::temp_dir(), v.dir.join("escape")).unwrap();
    assert!(fs.resolve("C:\\escape\\new-file").is_err());
    assert!(fs.resolve("C:\\escape").is_err());
}

#[test]
fn directory_queries_and_enumeration() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("one.cf"), b"cfg").unwrap();
    std::fs::write(v.dir.join("two.cf"), b"cfg2").unwrap();
    v.wide(DATA, "C:\\*.cf");
    let h = v.k("FindFirstFileW", &[DATA, DATA + 256]);
    assert_ne!(h, u32::MAX);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "one.cf");
    assert_eq!(v.k("FindNextFileW", &[h, DATA + 256]), 1);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "two.cf");
    assert_eq!(v.k("FindNextFileW", &[h, DATA + 256]), 0);
    assert_eq!(v.k("GetLastError", &[]), 18);
    assert_eq!(v.k("FindClose", &[h]), 1);
    assert_eq!(v.k("GetCurrentDirectoryW", &[2, DATA + 256]), 4);
    assert_eq!(v.k("GetCurrentDirectoryW", &[4, DATA + 256]), 3);
    assert_eq!(v.mem.read_wstr(DATA + 256, 4).unwrap(), "C:\\");
    let h = v.k("FindFirstFileExW", &[DATA, 1, DATA + 256, 0, 0, 2]);
    assert_ne!(h, u32::MAX);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "one.cf");
    assert_eq!(v.k("FindClose", &[h]), 1);
    assert_eq!(
        v.k("FindFirstFileExW", &[DATA, 2, DATA + 256, 0, 0, 0]),
        u32::MAX
    );
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn x86_slist_init_and_flush_preserve_sequence_and_return_chain() {
    let mut v = Vm::new();
    v.mem.write_u64(DATA, u64::MAX).unwrap();
    v.k("InitializeSListHead", &[DATA]);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 0);
    v.mem
        .write_u64(DATA, (0xffffu64 << 48) | (2u64 << 32) | (DATA + 16) as u64)
        .unwrap();
    assert_eq!(v.k("InterlockedFlushSList", &[DATA]), DATA + 16);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 0); // sequence wrapped, depth/pointer cleared
    assert_eq!(v.k("InterlockedFlushSList", &[DATA]), 0);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 1u64 << 48);
}

#[test]
fn thread_local_values_and_last_error_are_independent() {
    let mut v = Vm::new();
    let slot = v.k("TlsAlloc", &[]);
    let fls = v.k("FlsAlloc", &[0]);
    v.k("TlsSetValue", &[slot, 123]);
    v.k("FlsSetValue", &[fls, 456]);
    v.k("SetLastError", &[99]);
    let h = v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    assert_ne!(h, 0);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("GetLastError", &[]), 0);
    assert_eq!(v.k("TlsGetValue", &[slot]), 0);
    assert_eq!(v.k("FlsGetValue", &[fls]), 0);
    v.k("TlsSetValue", &[slot, 789]);
    v.k("SetLastError", &[88]);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("GetLastError", &[]), 99);
    assert_eq!(v.k("TlsGetValue", &[slot]), 123);
    assert_eq!(v.k("FlsGetValue", &[fls]), 456);
}

#[test]
fn interlocked_compare_exchange_uses_comparand_then_exchange() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 17).unwrap();
    assert_eq!(v.k("InterlockedCompareExchange", &[DATA, 42, 17]), 17);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 42);
    assert_eq!(v.k("InterlockedCompareExchange", &[DATA, 10, 17]), 42);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 42);
}

#[test]
fn software_seh_continues_with_modified_context_and_rejects_noncontinuable() {
    let mut v = Vm::new();
    // mov eax,[esp+12]; mov [eax+176],0x12345678; xor eax,eax; ret
    v.mem
        .write_bytes(
            CODE,
            &[
                0x8b, 0x44, 0x24, 0x0c, 0xc7, 0x80, 0xb0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0x31,
                0xc0, 0xc3,
            ],
        )
        .unwrap();
    v.mem.write_u32(DATA, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 4, CODE).unwrap();
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    assert_eq!(v.k("RaiseException", &[0x1234, 0, 0, 0]), 0x12345678);
    assert_eq!(v.cpu.eip, RET);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
    let error = v
        .try_call("kernel32.dll", "RaiseException", &[0x1234, 1, 0, 0])
        .unwrap_err();
    assert!(error.contains("noncontinuable"));
}

#[test]
fn unhandled_filter_is_registered_invoked_and_prior_filter_returned() {
    let mut v = Vm::new();
    // Filter returns EXCEPTION_CONTINUE_EXECUTION (-1), stdcall one argument.
    v.mem
        .write_bytes(CODE, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc2, 4, 0])
        .unwrap();
    assert_eq!(v.k("SetUnhandledExceptionFilter", &[CODE]), 0);
    v.k("RaiseException", &[0x1234, 0, 0, 0]);
    assert_eq!(v.cpu.eip, RET);
    assert_eq!(v.k("SetUnhandledExceptionFilter", &[0]), CODE);
    let error = v
        .try_call("kernel32.dll", "RaiseException", &[0xe06d7363, 1, 0, 0])
        .unwrap_err();
    assert!(error.contains("unhandled guest exception"));
}

#[test]
fn seh_unwind_cleans_inner_frames_keeps_target_and_restores_context() {
    let mut v = Vm::new();
    let first = STACK_BASE + STACK_SIZE - 128;
    let target = first + 32;
    // Each handler records its own frame, current FS head, flags and code.
    for (i, frame) in [first, first + 16].into_iter().enumerate() {
        let handler = CODE + i as u32 * 64;
        let out = DATA + i as u32 * 16;
        let mut code = vec![0x8b, 0x54, 0x24, 8, 0x89, 0x15];
        code.extend(out.to_le_bytes());
        code.extend([0x64, 0xa1, 0, 0, 0, 0, 0xa3]);
        code.extend((out + 4).to_le_bytes());
        code.extend([0x8b, 0x54, 0x24, 4, 0x8b, 0x42, 4, 0xa3]);
        code.extend((out + 8).to_le_bytes());
        code.extend([0x8b, 0x02, 0xa3]);
        code.extend((out + 12).to_le_bytes());
        code.extend([0xb8, 1, 0, 0, 0, 0xc3]);
        v.mem.write_bytes(handler, &code).unwrap();
        v.mem.write_u32(frame, frame + 16).unwrap();
        v.mem.write_u32(frame + 4, handler).unwrap();
    }
    v.mem.write_u32(target, u32::MAX).unwrap();
    v.mem.write_u32(target + 4, CODE + 0x500).unwrap();
    v.mem.write_u8(CODE + 0x500, 0xcc).unwrap(); // target must never be invoked
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    v.mem.write_u32(DATA + 64, 0x1234).unwrap();
    v.mem.write_u32(DATA + 68, 1).unwrap();
    v.cpu.regs[3] = 0xfeed_beef;
    assert_eq!(
        v.k("RtlUnwind", &[target, CODE + 0x500, DATA + 64, 0xface]),
        0xface
    );
    assert_eq!(v.cpu.regs[3], 0xfeed_beef);
    assert_eq!(v.cpu.eip, RET); // x86 TargetIp is ignored
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), target);
    for (i, frame) in [first, first + 16].into_iter().enumerate() {
        let out = DATA + i as u32 * 16;
        assert_eq!(v.mem.read_u32(out).unwrap(), frame);
        assert_eq!(v.mem.read_u32(out + 4).unwrap(), frame);
        assert_eq!(v.mem.read_u32(out + 8).unwrap(), 3);
        assert_eq!(v.mem.read_u32(out + 12).unwrap(), 0x1234);
    }
    // Null record/target generates STATUS_UNWIND and marks exit unwind.
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    v.mem.write_u32(first, u32::MAX).unwrap();
    v.k("RtlUnwind", &[0, 0, 0, 7]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 6);
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 0xc000_0027);
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), u32::MAX);
}

#[test]
fn seh_unwind_rejects_corrupt_or_absent_targets_before_cleanup() {
    let mut v = Vm::new();
    let first = STACK_BASE + STACK_SIZE - 128;
    v.mem.write_u32(first, u32::MAX).unwrap();
    v.mem.write_u32(first + 4, CODE).unwrap();
    v.mem.write_u8(CODE, 0xcc).unwrap(); // validation must not run this handler
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    assert!(v
        .try_call("kernel32.dll", "RtlUnwind", &[first + 16, 0, 0, 0])
        .unwrap_err()
        .contains("not in the chain"));
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), first);
    v.mem.write_u32(first, first).unwrap();
    assert!(v
        .try_call("ntdll.dll", "RtlUnwind", &[0, 0, 0, 0])
        .unwrap_err()
        .contains("cyclic"));
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    assert!(v
        .try_call("kernel32.dll", "RtlUnwind", &[0, 0, 0, 0])
        .unwrap_err()
        .contains("invalid SEH unwind frame"));
}

#[test]
fn seh_catch_transfer_escapes_raise_and_initializer_host_callbacks() {
    for initializer in [false, true] {
        let mut v = Vm::new();
        let first = STACK_BASE + STACK_SIZE - 128;
        let target = first + 32;
        let unwind = v.api.register("kernel32.dll", Some("RtlUnwind"), None);
        let raise = v.api.register("kernel32.dll", Some("RaiseException"), None);
        v.mem.write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc3]).unwrap();
        v.mem.write_u32(first, target).unwrap();
        v.mem.write_u32(first + 4, CODE).unwrap();
        v.mem.write_u32(target, u32::MAX).unwrap();
        v.mem.write_u32(target + 4, CODE + 64).unwrap();
        v.mem.write_u32(TEB_ADDR, first).unwrap();
        // Outer search handler: global unwind, then discard the callback stack
        // and jump to a catch outside both RaiseException and _initterm.
        let mut handler = vec![0x8b, 0x44, 0x24, 4, 0x6a, 0, 0x50, 0x6a, 0, 0x68];
        handler.extend(target.to_le_bytes());
        handler.push(0xb8);
        handler.extend(unwind.to_le_bytes());
        handler.extend([0xff, 0xd0, 0xbc]);
        handler.extend((target - 16).to_le_bytes());
        handler.push(0xb8);
        handler.extend((CODE + 256).to_le_bytes());
        handler.extend([0xff, 0xe0]);
        v.mem.write_bytes(CODE + 64, &handler).unwrap();
        let mut catch = vec![0xc7, 0x05];
        catch.extend(DATA.to_le_bytes());
        catch.extend(0xcafe_beefu32.to_le_bytes());
        catch.push(0xcc);
        v.mem.write_bytes(CODE + 256, &catch).unwrap();
        if initializer {
            let mut code = vec![0x6a, 0, 0x6a, 0, 0x6a, 1, 0x68];
            code.extend(0xe06d_7363u32.to_le_bytes());
            code.push(0xb8);
            code.extend(raise.to_le_bytes());
            code.extend([0xff, 0xd0, 0xcc]); // cannot return here
            v.mem.write_bytes(CODE + 512, &code).unwrap();
            v.mem.write_u32(DATA + 64, CODE + 512).unwrap();
            v.call("msvcrt.dll", "_initterm", &[DATA + 64, DATA + 68]);
        } else {
            v.k("RaiseException", &[0xe06d_7363, 1, 0, 0]);
        }
        assert_eq!(v.cpu.esp(), target - 16);
        assert_ne!(v.cpu.eip, RET); // no stub epilogue/saved callback restoration
        let outcome = v.cpu.run(
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            1_000_000,
        );
        assert!(
            matches!(outcome, wire::cpu::RunOutcome::Crashed(ref e) if e.contains("breakpoint"))
        );
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xcafe_beef);
        assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), target);
    }
}

#[test]
fn window_classes_keep_metadata_support_atoms_and_share_ansi_unicode_names() {
    let mut v = Vm::new();
    v.wide(DATA + 128, "WireWindow");
    v.mem.write_bytes(DATA + 160, b"wirewindow\0").unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, DATA + 128, DATA + 256]
        ),
        0
    );
    assert_eq!(v.state.last_error, 1411);
    let words = [48, 3, CODE, 4, 8, 0x400000, 2, 3, 4, 0, DATA + 128, 5];
    for (i, value) in words.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, value).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassExW", &[DATA]);
    assert!((0xc000..=0xffff).contains(&atom));
    assert_eq!(v.call("user32.dll", "RegisterClassExW", &[DATA]), 0);
    assert_eq!(v.state.last_error, 1410);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExA",
            &[0x400000, DATA + 160, DATA + 256]
        ),
        1
    );
    for (i, value) in words.into_iter().enumerate().filter(|(i, _)| *i != 10) {
        assert_eq!(v.mem.read_u32(DATA + 256 + i as u32 * 4).unwrap(), value);
    }
    let name = v.mem.read_u32(DATA + 296).unwrap();
    assert_eq!(v.mem.read_cstr(name, 256).unwrap(), "WireWindow");
    assert_eq!(
        v.call("user32.dll", "GetClassInfoW", &[0x400000, atom, DATA + 320]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 324).unwrap(), CODE);
    let name = v.mem.read_u32(DATA + 356).unwrap();
    assert_eq!(v.mem.read_wstr(name, 256).unwrap(), "WireWindow");
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExA",
            &[0x500000, atom, DATA + 256]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "UnregisterClassA", &[atom, 0x400000]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, DATA + 128, DATA + 256]
        ),
        0
    );
    // WNDCLASS (40 bytes) has no cbSize/hIconSm and remains interoperable.
    for (i, value) in words[1..11].iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, *value).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassW", &[DATA]);
    assert_ne!(atom, 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, atom, DATA + 256]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 300).unwrap(), 0);
}

fn test_window(v: &mut Vm, wndproc: u32) -> u32 {
    v.wide(DATA + 128, "CallbackWindow");
    let words = [48, 0, wndproc, 0, 8, 0x400000, 0, 0, 0, 0, DATA + 128, 0];
    for (i, n) in words.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassExW", &[DATA]);
    assert_ne!(atom, 0);
    v.call(
        "user32.dll",
        "CreateWindowExW",
        &[0, atom, 0, 0, 3, 4, 640, 480, 0, 0, 0x400000, 0x1234],
    )
}

#[test]
fn windows_invoke_creation_forwarding_and_destruction_callbacks() {
    let mut v = Vm::new();
    // Record message/count; return TRUE only for WM_NCCREATE, stdcall 16.
    let mut proc = vec![0x8b, 0x44, 0x24, 8, 0xa3];
    proc.extend((DATA + 256).to_le_bytes());
    proc.extend([0xff, 0x05]);
    proc.extend((DATA + 260).to_le_bytes());
    proc.extend([0x8b, 0x54, 0x24, 16, 0x89, 0x15]);
    proc.extend((DATA + 264).to_le_bytes());
    proc.extend([
        0x3d, 0x81, 0, 0, 0, 0x0f, 0x94, 0xc0, 0x0f, 0xb6, 0xc0, 0xc2, 16, 0,
    ]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    let hwnd = test_window(&mut v, CODE);
    assert_ne!(hwnd, 0);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 260).unwrap(), 2);
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "IsWindowUnicode", &[hwnd]), 1);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetWindowLongA",
            &[hwnd, (-21i32) as u32, 0xfeed]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "GetWindowLongW", &[hwnd, (-21i32) as u32]),
        0xfeed
    );
    assert_eq!(
        v.call("user32.dll", "SetWindowLongW", &[hwnd, 4, 0xabcd]),
        0
    );
    assert_eq!(v.call("user32.dll", "GetWindowLongW", &[hwnd, 4]), 0xabcd);
    assert_eq!(v.call("user32.dll", "GetWindowLongW", &[hwnd, 8]), 0);
    assert_eq!(v.state.last_error, 1413);
    assert_eq!(
        v.call("user32.dll", "GetClientRect", &[hwnd, DATA + 320]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 328).unwrap(), 640);
    assert_eq!(v.mem.read_u32(DATA + 332).unwrap(), 480);
    v.mem
        .write_bytes(CODE + 128, &[0x8b, 0x44, 0x24, 12, 0x40, 0xc2, 16, 0])
        .unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "SetWindowLongW",
            &[hwnd, (-4i32) as u32, CODE + 128]
        ),
        CODE
    );
    assert_eq!(
        v.call("user32.dll", "SendMessageW", &[hwnd, 0x401, 42, 0]),
        43
    );
    assert_eq!(
        v.call("user32.dll", "CallWindowProcW", &[CODE, hwnd, 0x402, 0, 0]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 0x402);
    assert_eq!(
        v.call("user32.dll", "UnregisterClassW", &[DATA + 128, 0x400000]),
        0
    );
    assert_eq!(v.state.last_error, 1412);
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, CODE],
    );
    let before = v.mem.read_u32(DATA + 260).unwrap();
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 0x82);
    assert_eq!(v.mem.read_u32(DATA + 260).unwrap(), before + 2);
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 0);
    // An ANSI caller creating a Unicode class supplies a converted CREATESTRUCT.
    v.mem.write_bytes(DATA + 160, b"mixed title\0").unwrap();
    v.mem.write_bytes(DATA + 192, b"MissingClass\0").unwrap();
    let another = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[
            0,
            DATA + 160 + 32,
            DATA + 160,
            0,
            0,
            0,
            10,
            10,
            0,
            0,
            0x400000,
            0x2222,
        ],
    );
    // Unknown class is rejected rather than creating a generic placeholder.
    assert_eq!(another, 0);
    v.mem.write_bytes(DATA + 192, b"CallbackWindow\0").unwrap();
    let another = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[
            0,
            DATA + 192,
            DATA + 160,
            0,
            0,
            0,
            10,
            10,
            0,
            0,
            0x400000,
            0x2222,
        ],
    );
    assert_ne!(another, 0);
    assert_eq!(v.call("user32.dll", "IsWindowUnicode", &[another]), 1);
    let cs = v.mem.read_u32(DATA + 264).unwrap();
    assert_eq!(v.mem.read_u32(cs).unwrap(), 0x2222);
    let title = v.mem.read_u32(cs + 36).unwrap();
    assert_eq!(v.mem.read_wstr(title, 64).unwrap(), "mixed title");
    v.call("user32.dll", "DestroyWindow", &[another]);
    assert_eq!(
        v.call("user32.dll", "UnregisterClassW", &[DATA + 128, 0x400000]),
        1
    );
    v.mem.write_bytes(CODE, &[0x31, 0xc0, 0xc2, 16, 0]).unwrap();
    assert_eq!(test_window(&mut v, CODE), 0);
    assert!(v.state.user32.windows.is_empty());
}

#[test]
fn message_filters_quit_and_timers_preserve_queue_and_callback_semantics() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    let msg = DATA + 512;
    v.mem.write_u32(msg, 0xeeee).unwrap();
    assert_eq!(v.call("user32.dll", "PeekMessageW", &[msg, 0, 0, 0, 1]), 0);
    assert_eq!(v.mem.read_u32(msg).unwrap(), 0xeeee);
    v.call("user32.dll", "PostMessageW", &[hwnd, 0x400, 11, 22]);
    v.call("user32.dll", "PostMessageA", &[hwnd, 0x401, 33, 44]);
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0x401, 0x401, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 33);
    assert_eq!(v.call("user32.dll", "GetMessageA", &[msg, 0, 0, 0]), 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x400);
    assert_eq!(v.call("user32.dll", "PeekMessageW", &[msg, 0, 0, 0, 1]), 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x401);
    assert_eq!(v.call("user32.dll", "SetTimer", &[hwnd, 7, 50, 0]), 7);
    assert_eq!(v.call("user32.dll", "SetTimer", &[hwnd, 1, 50, 0]), 1);
    let automatic = v.call("user32.dll", "SetTimer", &[hwnd, 0, 50, 0]);
    assert_ne!(automatic, 1);
    v.call("user32.dll", "KillTimer", &[hwnd, automatic]);
    v.call("user32.dll", "KillTimer", &[hwnd, 1]);
    v.state
        .user32
        .timers
        .get_mut(&(1, hwnd, 7))
        .unwrap()
        .deadline = std::time::Instant::now();
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x113);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 7);
    v.call("user32.dll", "KillTimer", &[hwnd, 7]);
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 1]),
        1
    );
    assert_eq!(v.call("user32.dll", "DispatchMessageW", &[msg]), 0);
    let mut proc = vec![0x8b, 0x44, 0x24, 12, 0xa3];
    proc.extend((DATA + 400).to_le_bytes());
    proc.extend([0x8b, 0x44, 0x24, 16, 0xa3]);
    proc.extend((DATA + 404).to_le_bytes());
    proc.extend([0x31, 0xc0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    v.call("user32.dll", "SetTimer", &[hwnd, 9, 50, CODE]);
    v.state
        .user32
        .timers
        .get_mut(&(1, hwnd, 9))
        .unwrap()
        .deadline = std::time::Instant::now();
    v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 1]);
    let time = v.mem.read_u32(msg + 16).unwrap();
    v.call("user32.dll", "DispatchMessageW", &[msg]);
    assert_eq!(v.mem.read_u32(DATA + 400).unwrap(), 9);
    assert_eq!(v.mem.read_u32(DATA + 404).unwrap(), time);
    v.call("user32.dll", "PostQuitMessage", &[17]);
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[msg, hwnd, 0x900, 0x900]),
        0
    );
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x12);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 17);
    v.call("user32.dll", "DestroyWindow", &[hwnd]);
    assert!(v.state.user32.timers.is_empty());
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[msg, hwnd, 0, 0]),
        u32::MAX
    );
}

#[test]
fn blocked_message_thread_resumes_after_worker_posts_without_fake_quit() {
    let mut v = Vm::new();
    let post = v
        .api
        .register("user32.dll", Some("PostThreadMessageW"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitThread"), None);
    let mut worker = vec![0x6a, 44, 0x6a, 33, 0x68];
    worker.extend(0x501u32.to_le_bytes());
    worker.extend([0x6a, 1, 0xb8]);
    worker.extend(post.to_le_bytes());
    worker.extend([0xff, 0xd0, 0x6a, 0, 0xb8]);
    worker.extend(exit.to_le_bytes());
    worker.extend([0xff, 0xd0, 0xcc]);
    v.mem.write_bytes(CODE, &worker).unwrap();
    v.k("CreateThread", &[0, 0, CODE, 0, 0, DATA]);
    let get = v.api.register("user32.dll", Some("GetMessageW"), None);
    let msg = DATA + 512;
    v.cpu.eip = get;
    let sp = v.cpu.esp();
    v.mem.write_u32(sp, RET).unwrap();
    for (i, n) in [msg, 0, 0, 0].into_iter().enumerate() {
        v.mem.write_u32(sp + 4 + i as u32 * 4, n).unwrap();
    }
    v.mem.write_u8(RET, 0xcc).unwrap();
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::Crashed(ref e) if e.contains("breakpoint")));
    assert_eq!(v.sched.current_tid(), 1);
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x501);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 33);
    assert_eq!(v.mem.read_u32(msg + 12).unwrap(), 44);
}

#[test]
fn resources_load_real_utf16_strings_and_bound_lengths() {
    let mut v = Vm::new();
    let base = v.state.exe_base;
    v.mem.commit(base, 0x10000).unwrap();
    v.mem.write_u32(base + 0x3c, 0x80).unwrap();
    v.mem.write_u32(base + 0x80, 0x4550).unwrap();
    v.mem.write_u32(base + 0x80 + 24 + 112, 0x1000).unwrap();
    v.mem.write_u32(base + 0x80 + 24 + 116, 0x1000).unwrap();
    let root = base + 0x1000;
    for off in [0, 24, 48] {
        v.mem.write_u16(root + off + 14, 1).unwrap();
    }
    for (off, value) in [
        (16, 6),
        (20, 0x80000018),
        (40, 1),
        (44, 0x80000030),
        (64, 0x411),
        (68, 72),
        (72, 0x2000),
        (76, 36),
    ] {
        v.mem.write_u32(root + off, value).unwrap();
    }
    v.mem.write_u16(base + 0x2000, 2).unwrap();
    v.wide(base + 0x2002, "日本");
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 10]), 2);
    assert_eq!(v.mem.read_wstr(DATA, 10).unwrap(), "日本");
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236); // ret + four stdcall args
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 0]), 2);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), base + 0x2002);
    assert_eq!(v.call("user32.dll", "LoadStringA", &[base, 0, DATA, 10]), 4);
    let mut b = [0; 5];
    v.mem.read_bytes(DATA, &mut b).unwrap();
    assert_eq!(b, [0x93, 0xfa, 0x96, 0x7b, 0]);
    let res = v.k("FindResourceW", &[base, 1, 6]);
    assert_eq!(res, root + 72);
    assert_eq!(v.k("SizeofResource", &[base, res]), 36);
    assert_eq!(v.k("LoadResource", &[base, res]), base + 0x2000);
    v.mem.write_u16(base + 0x2000, 0xffff).unwrap();
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 10]), 0);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0);
}

#[test]
fn format_message_allocation_and_local_free_share_real_heap() {
    let mut v = Vm::new();
    let count = v.k("FormatMessageW", &[0x1300, 0, 2, 0, DATA, 0, 0]);
    assert!(count > 0);
    let p = v.mem.read_u32(DATA).unwrap();
    assert_ne!(p, 0);
    assert_eq!(
        v.mem.read_wstr(p, 100).unwrap(),
        "The system cannot find the file specified.\r\n"
    );
    assert_eq!(v.k("LocalFree", &[p]), 0);
    assert_eq!(v.k("HeapValidate", &[v.state.process_heap, 0, p]), 0);
}

#[test]
fn wait_all_acquires_objects_atomically() {
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 1, 0]);
    let sem = v.k("CreateSemaphoreW", &[0, 0, 1, 0]);
    v.mem.write_u32(DATA, event).unwrap();
    v.mem.write_u32(DATA + 4, sem).unwrap();
    assert_eq!(v.k("WaitForMultipleObjects", &[2, DATA, 1, 0]), 0x102);
    assert!(matches!(
        v.state.handles.get(&event),
        Some(HandleObj::Event { signaled: true, .. })
    ));
    assert_eq!(v.k("ReleaseSemaphore", &[sem, 1, 0]), 1);
    assert_eq!(v.k("WaitForMultipleObjects", &[2, DATA, 1, 0]), 0);
    assert!(matches!(
        v.state.handles.get(&event),
        Some(HandleObj::Event {
            signaled: false,
            ..
        })
    ));
    assert_eq!(v.k("WaitForSingleObject", &[sem, 0]), 0x102);
    assert_eq!(v.k("SetEvent", &[event]), 1);
    assert_eq!(v.k("WaitForSingleObjectEx", &[event, 0, 1]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 240); // stdcall: return + 3 arguments
    assert_eq!(v.k("WaitForSingleObjectEx", &[event, 0, 0]), 0x102);
    assert_eq!(v.k("WaitForSingleObjectEx", &[0xdead, 0, 0]), u32::MAX);
    assert_eq!(v.k("GetLastError", &[]), 6);
}

#[test]
fn blocked_thread_resumes_after_worker_signals_and_exits() {
    fn call(code: &mut Vec<u8>, stub: u32, args: &[u32]) {
        for &a in args.iter().rev() {
            code.push(0x68);
            code.extend(a.to_le_bytes());
        }
        code.push(0xb8);
        code.extend(stub.to_le_bytes());
        code.extend([0xff, 0xd0]);
    }
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    let set = v.api.register("kernel32.dll", Some("SetEvent"), None);
    let mut worker = Vec::new();
    call(&mut worker, set, &[event]);
    worker.extend([0xb8, 42, 0, 0, 0, 0xc2, 4, 0]);
    v.mem.write_bytes(CODE + 0x400, &worker).unwrap();
    let h = v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    let wait = v
        .api
        .register("kernel32.dll", Some("WaitForSingleObject"), None);
    let get_exit = v
        .api
        .register("kernel32.dll", Some("GetExitCodeThread"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut main = Vec::new();
    call(&mut main, wait, &[event, u32::MAX]);
    main.push(0xa3);
    main.extend((DATA + 16).to_le_bytes());
    call(&mut main, get_exit, &[h, DATA + 20]);
    call(&mut main, exit, &[0]);
    v.mem.write_bytes(CODE, &main).unwrap();
    v.cpu.eip = CODE;
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        10_000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::Exited(0)));
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 0);
    assert_eq!(v.mem.read_u32(DATA + 20).unwrap(), 42);
}

#[test]
fn timeslice_wakes_expired_waiter_even_while_other_thread_spins() {
    use wire::thread::{ThreadState, WaitTarget, Wake};
    let mut v = Vm::new();
    v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    v.sched.threads[1].state = ThreadState::Waiting {
        target: WaitTarget::Sleep,
        deadline: Some(std::time::Instant::now()),
    };
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.sched.current, 1);
    assert_eq!(v.sched.take_wake(), Some(Wake::Completed(0x102)));
    assert_eq!(v.cpu.mxcsr, 0x1f80);
    assert_eq!(v.cpu.fpu.cw, 0x037f);
}

#[test]
fn worker_teb_slots_do_not_alias_main_teb_or_peb() {
    let mut v = Vm::new();
    v.mem.commit(wire::loader::PEB_ADDR, 0x1000).unwrap();
    v.mem.write_u32(wire::loader::PEB_ADDR, 0xcafe).unwrap();
    v.mem.write_u32(TEB_ADDR + 0x34, 0xbeef).unwrap();
    for _ in 0..40 {
        v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    }
    assert_eq!(v.mem.read_u32(wire::loader::PEB_ADDR).unwrap(), 0xcafe);
    assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 0xbeef);
    for t in v.sched.threads.iter().skip(1) {
        assert_ne!(t.teb, TEB_ADDR);
        assert_ne!(t.teb, wire::loader::PEB_ADDR);
        assert_eq!(v.mem.read_u32(t.teb + 0x24).unwrap(), t.tid);
    }
}

#[test]
fn x87_stack_and_rounding_survive_thread_switches() {
    let mut v = Vm::new();
    v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    v.mem.write_bytes(CODE, &[0xd9, 0xe8]).unwrap();
    v.cpu.eip = CODE;
    v.cpu.step(&mut v.mem).unwrap();
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.tags, 0xffff);
    v.cpu.fpu.cw = 0xb7f;
    v.mem.write_bytes(CODE + 0x400, &[0xd9, 0xeb]).unwrap();
    v.cpu.step(&mut v.mem).unwrap();
    let worker = v.cpu.fpu;
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.st_bits(0), 0x3fff_8000_0000_0000_0000);
    assert_eq!(v.cpu.fpu.cw, 0x37f);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.regs, worker.regs);
    assert_eq!(v.cpu.fpu.tags, worker.tags);
    assert_eq!(v.cpu.fpu.sw, worker.sw);
    assert_eq!(v.cpu.fpu.cw, worker.cw);
}
#[test]
fn seh_context_preserves_and_allows_editing_x87_stack() {
    let mut v = Vm::new();
    v.mem.write_bytes(CODE + 0x400, &[0xd9, 0xe8]).unwrap();
    v.cpu.eip = CODE + 0x400;
    v.cpu.step(&mut v.mem).unwrap();
    v.cpu.fpu.cw = 0xb7f;
    // FNINIT; ctx=[esp+12]; FLDZ; FSTP m80[ctx+56]; tag[ctx]=zero;
    // return ExceptionContinueExecution. Callback clobbers the live FP state.
    v.mem
        .write_bytes(
            CODE,
            &[
                0xdb, 0xe3, 0x8b, 0x44, 0x24, 0x0c, 0xd9, 0xee, 0xdb, 0x78, 0x38, 0xc7, 0x40, 0x24,
                0xff, 0x7f, 0, 0, 0x31, 0xc0, 0xc3,
            ],
        )
        .unwrap();
    v.mem.write_u32(DATA, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 4, CODE).unwrap();
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    v.k("RaiseException", &[0x1234, 0, 0, 0]);
    assert_eq!(v.cpu.fpu.cw, 0xb7f);
    assert_eq!(v.cpu.fpu.top(), 7);
    assert_eq!(v.cpu.fpu.tags, 0x7fff);
    assert_eq!(v.cpu.fpu.st_bits(0), 0);
}

#[test]
fn system_profile_memory_structures_and_monotonic_clocks() {
    let mut v = Vm::new();
    v.k("GetNativeSystemInfo", &[DATA]);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0); // x86, irrespective of host
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 4096);
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 20).unwrap(), 1);
    v.mem.write_u32(DATA + 128, 32).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA + 128]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
    v.mem.write_u32(DATA + 128, 64).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA + 128]), 1);
    let available = v.mem.read_u64(DATA + 144).unwrap();
    v.k("HeapAlloc", &[v.state.process_heap, 0, 65536]);
    v.k("GlobalMemoryStatusEx", &[DATA + 128]);
    assert!(v.mem.read_u64(DATA + 144).unwrap() <= available - 65536);
    v.k("GlobalMemoryStatus", &[DATA + 256]);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 32);
    assert_eq!(
        v.mem.read_u32(DATA + 264).unwrap() as u64,
        v.mem.read_u64(DATA + 136).unwrap()
    );
    v.state.start =
        std::time::Instant::now() - std::time::Duration::from_millis((1u64 << 32) + 1000);
    let ticks = v.k("GetTickCount64", &[]);
    assert_eq!(v.cpu.regs[2], 1);
    assert!((1000..2000).contains(&ticks));
    let mm = v.call("winmm.dll", "timeGetTime", &[]);
    let ordinary = v.k("GetTickCount", &[]);
    assert!(ordinary.wrapping_sub(mm) < 100);
    assert_eq!(v.call("winmm.dll", "timeGetDevCaps", &[DATA, 7]), 97);
    assert_eq!(v.call("winmm.dll", "timeGetDevCaps", &[DATA, 8]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    assert_eq!(v.call("winmm.dll", "timeBeginPeriod", &[0]), 97);
    assert_eq!(v.call("winmm.dll", "timeBeginPeriod", &[1]), 0);
    v.k("GetSystemTime", &[DATA]);
    assert!(v.mem.read_u16(DATA).unwrap() >= 2026);
    assert!((1..=12).contains(&v.mem.read_u16(DATA + 2).unwrap()));
}
#[test]
fn nls_case_mapping_lengths_buffers_and_crt_character_types() {
    let mut v = Vm::new();
    v.wide(DATA, "aZ 9日本");
    assert_eq!(
        v.k("LCMapStringEx", &[0, 0x200, DATA, u32::MAX, 0, 0, 0, 0, 0]),
        7
    );
    assert_eq!(
        v.k(
            "LCMapStringEx",
            &[0, 0x200, DATA, u32::MAX, DATA + 64, 7, 0, 0, 0]
        ),
        7
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 7).unwrap(), "AZ 9日本");
    assert_eq!(
        v.k(
            "LCMapStringW",
            &[0x411, 0x100, DATA + 64, u32::MAX, DATA + 64, 7]
        ),
        7
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 7).unwrap(), "az 9日本");
    assert_eq!(
        v.k(
            "LCMapStringEx",
            &[0, 0x200, DATA, u32::MAX, DATA + 128, 6, 0, 0, 0]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 122);
    assert_eq!(v.k("GetStringTypeW", &[1, DATA, u32::MAX, DATA + 128]), 1);
    assert_eq!(v.mem.read_u16(DATA + 128).unwrap() & 0x182, 0x182); // lower, alpha, hex 'a'
    assert_eq!(v.mem.read_u16(DATA + 132).unwrap() & 0x48, 0x48); // blank/space
    assert_eq!(v.mem.read_u16(DATA + 134).unwrap() & 0x84, 0x84); // digit/hex
    assert_eq!(v.mem.read_u16(DATA + 136).unwrap() & 0x100, 0x100); // Japanese letter
    assert_eq!(v.mem.read_u16(DATA + 140).unwrap() & 0x20, 0x20); // explicit NUL count
    assert_eq!(v.k("GetStringTypeW", &[3, DATA, 1, DATA + 128]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1004);
}
