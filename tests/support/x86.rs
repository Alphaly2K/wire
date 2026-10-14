//! Shared PE32 API/process behavioral harness; no test target of its own.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use wire::api::{self, ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::Cpu;
use wire::fs::GuestFs;
use wire::loader::{STACK_BASE, STACK_SIZE, TEB_ADDR};
use wire::mem::GuestMem;
use wire::thread::Sched;
pub(crate) const DATA: u32 = 0x20000;
pub(crate) const CODE: u32 = 0x10000;
pub(crate) const RET: u32 = CODE + 0x800;

pub(crate) struct Vm {
    pub(crate) mem: GuestMem,
    pub(crate) cpu: Cpu,
    pub(crate) api: ApiTable,
    pub(crate) state: ApiState,
    pub(crate) log: ApiLogger,
    pub(crate) sched: Sched,
    pub(crate) dir: PathBuf,
}
impl Vm {
    pub(crate) fn new() -> Self {
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
    pub(crate) fn call(&mut self, dll: &str, name: &str, args: &[u32]) -> u32 {
        self.try_call(dll, name, args).unwrap()
    }
    pub(crate) fn try_call(&mut self, dll: &str, name: &str, args: &[u32]) -> Result<u32, String> {
        let addr = self.api.register(dll, Some(name), None);
        self.cpu.eip = addr;
        self.cpu.set_esp(STACK_BASE + STACK_SIZE - 256);
        let sp = self.cpu.esp();
        self.mem.write_u32(sp, RET).unwrap();
        for i in 0..16 {
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
    pub(crate) fn k(&mut self, name: &str, args: &[u32]) -> u32 {
        self.call("kernel32.dll", name, args)
    }
    pub(crate) fn wide(&self, p: u32, text: &str) {
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
pub(crate) fn test_dll(import: &str, attach_ok: bool, value: u32) -> Vec<u8> {
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

pub(crate) fn dll_export(v: &mut Vm, module: u32, name: &str) -> u32 {
    v.mem
        .write_bytes(DATA + 0x100, format!("{name}\0").as_bytes())
        .unwrap();
    v.k("GetProcAddress", &[module, DATA + 0x100])
}

pub(crate) fn test_window(v: &mut Vm, wndproc: u32) -> u32 {
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

pub(crate) fn d3d_com(v: &mut Vm, object: u32, slot: u32, args: &[u32]) -> u32 {
    let table = v.mem.read_u32(object).unwrap();
    let stub = v.mem.read_u32(table + slot * 4).unwrap();
    let mut all = vec![object];
    all.extend_from_slice(args);
    let result = v
        .cpu
        .call_guest(
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            stub,
            &all,
            1_000_000,
        )
        .unwrap();
    match result {
        wire::cpu::GuestCallOutcome::Returned(n) => n,
        _ => panic!("COM control transfer escaped"),
    }
}
