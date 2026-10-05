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
