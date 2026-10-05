//! Actual x86 -> existing IR -> direct Wasm lowering, compared with both wire
//! interpreters and run through the public macOS JSC C API. No guest MIPS are
//! attributed to the synthetic gateway/IO measurement helpers.
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use wire::api::{ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::{Cpu, RunOutcome};
use wire::ir::wasm::{BUDGET, DONE, EIP_SLOT, INVALID_ENTRY, RETIRED_SLOT, VREG_BYTES};
use wire::ir::Image;
use wire::mem::GuestMem;
use wire::thread::Sched;
const BASE: u32 = 0x10000;

fn program(iterations: u32, flags: bool) -> (Vec<u8>, u32) {
    let mut code = vec![0xb9];
    code.extend(iterations.to_le_bytes());
    code.extend([0xba, 1, 0, 0, 0, 0x31, 0xc0]);
    let start = code.len();
    if flags {
        // ADD ADC SBB OR AND SUB XOR CMP TEST INC DEC, then JNZ.
        code.extend([
            0x01, 0xd0, 0x11, 0xfb, 0x19, 0xc5, 0x09, 0xc6, 0x21, 0xd7, 0x29, 0xdd, 0x31, 0xc3,
            0x39, 0xd0, 0x85, 0xd6, 0x42, 0x49,
        ]);
    } else {
        code.extend([0x01, 0xd0, 0x83, 0xc2, 1, 0x31, 0xc3, 0x39, 0xd0, 0x49]);
    }
    let relative = -((code.len() - start + 2) as i8);
    code.extend([0x75, relative as u8, 0xcc]);
    (code, if flags { 12 } else { 6 })
}
fn put(bank: &mut [u8; VREG_BYTES], slot: usize, value: u64) {
    bank[slot * 8..slot * 8 + 8].copy_from_slice(&value.to_le_bytes());
}
fn get(bank: &[u8; VREG_BYTES], slot: usize) -> u32 {
    u64::from_le_bytes(bank[slot * 8..slot * 8 + 8].try_into().unwrap()) as u32
}
fn initial(seed: u32) -> [u8; VREG_BYTES] {
    let mut bank = [0; VREG_BYTES];
    let mut x = seed;
    for slot in 0..64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        put(
            &mut bank,
            slot,
            if slot < 11 {
                x as u64
            } else {
                (x as u64) << 32 | !x as u64
            },
        );
    }
    put(&mut bank, 8, (0x202 | (seed & 0x8d5)) as u64);
    put(&mut bank, EIP_SLOT, BASE as u64);
    put(&mut bank, RETIRED_SLOT, 0);
    bank
}
fn reference(
    code: &[u8],
    init: &[u8; VREG_BYTES],
    budget: u32,
    ir: bool,
) -> ([u8; VREG_BYTES], f64) {
    let mut mem = GuestMem::new().unwrap();
    mem.commit(BASE, 0x10000).unwrap();
    mem.write_bytes(BASE, code).unwrap();
    let mut cpu = Cpu::new(BASE, 0, 0);
    for slot in 0..8 {
        cpu.regs[slot] = get(init, slot);
    }
    cpu.eflags = get(init, 8);
    cpu.profile_opcodes = false;
    if ir {
        cpu.install_ir(Image::from_code(BASE, code).unwrap(), &mem)
            .unwrap();
    }
    let mut api = ApiTable::new();
    let mut state = ApiState::new(BASE, "C:\\wasm-bench.exe".into(), 0, 0);
    let log_path =
        std::env::temp_dir().join(format!("wire-wasm-oracle-{}.log", std::process::id()));
    let mut log = ApiLogger::new(&log_path).unwrap();
    let mut sched = Sched::new();
    let tid = sched.alloc_tid();
    let handle = state.alloc_handle(HandleObj::Thread { tid });
    sched.add_main(tid, handle, 0);
    let start = Instant::now();
    let outcome = cpu.run(
        &mut mem,
        &mut api,
        &mut state,
        &mut log,
        &mut sched,
        budget as u64,
    );
    let elapsed = start.elapsed().as_secs_f64();
    assert!(matches!(outcome, RunOutcome::InstructionLimit));
    let mut result = *init;
    for slot in 0..8 {
        put(&mut result, slot, cpu.regs[slot] as u64);
    }
    put(&mut result, 8, cpu.eflags as u64);
    put(&mut result, EIP_SLOT, cpu.eip as u64);
    put(&mut result, RETIRED_SLOT, cpu.instr_count);
    drop(log);
    std::fs::remove_file(log_path).unwrap();
    (result, elapsed)
}
fn case(
    out: &mut Vec<u8>,
    budget: u32,
    reason: u32,
    initial: &[u8; VREG_BYTES],
    expected: &[u8; VREG_BYTES],
) {
    out.extend(budget.to_le_bytes());
    out.extend(reason.to_le_bytes());
    out.extend(initial);
    out.extend(expected);
}
fn main() {
    if !cfg!(target_os = "macos") {
        eprintln!("this runner uses the macOS JavaScriptCore framework");
        std::process::exit(2);
    }
    let iterations = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&n| (100..=100_000_000).contains(&n))
        .unwrap_or(10_000_000);
    let flags = std::env::args().nth(2).as_deref() == Some("flags");
    let (code, loop_count) = program(iterations, flags);
    let total = iterations
        .checked_mul(loop_count)
        .and_then(|n| n.checked_add(3))
        .unwrap();
    let image = Image::from_code(BASE, &code).unwrap();
    let start = Instant::now();
    let module = image.lower_wasm_loop().unwrap();
    println!(
        "AOT: {} x86 bytes -> {} Wasm bytes in {:.3}ms; guest region {:#x}..{:#x}",
        code.len(),
        module.bytes.len(),
        start.elapsed().as_secs_f64() * 1e3,
        module.entry,
        module.exit
    );
    let root = Path::new("wire-out/jsc");
    std::fs::create_dir_all(root).unwrap();
    let name = if flags { "flags" } else { "alu" };
    let module_path = root.join(format!("{name}.wasm"));
    let oracle_path = root.join(format!("{name}.oracle"));
    std::fs::write(&module_path, &module.bytes).unwrap();
    let mut cases = b"WJCASE01".to_vec();
    cases.extend(0u32.to_le_bytes());
    let mut case_count = 0u32;
    // All early boundaries, including budgets zero and stops within a loop;
    // varying seeds retain overflow/carry/sign/parity and unused state checks.
    for seed in [0x81234567, 0xffffffff, 0x12345678] {
        let init = initial(seed);
        for budget in 0..3 + 2 * loop_count + 1 {
            let (raw, _) = reference(&code, &init, budget, false);
            let (ir, _) = reference(&code, &init, budget, true);
            assert_eq!(raw, ir);
            case(&mut cases, budget, BUDGET, &init, &raw);
            case_count += 1;
        }
    }
    let mut wrong_entry = initial(0xdeadbeef);
    put(&mut wrong_entry, EIP_SLOT, (BASE + 1) as u64);
    case(&mut cases, 1, INVALID_ENTRY, &wrong_entry, &wrong_entry);
    case_count += 1;
    let init = initial(0x81234567);
    let mut expected = None;
    for round in 0..6 {
        let ir = round % 2 == 1;
        let (state, elapsed) = reference(&code, &init, total, ir);
        if let Some(previous) = expected {
            assert_eq!(state, previous);
        } else {
            expected = Some(state);
        }
        println!(
            "{} {:.2} guest MIPS ({:.6}s, {} retired)",
            if ir { "IR " } else { "x86" },
            total as f64 / elapsed / 1e6,
            elapsed,
            total
        );
    }
    case(&mut cases, total, DONE, &init, &expected.unwrap());
    case_count += 1;
    cases[8..12].copy_from_slice(&case_count.to_le_bytes());
    std::fs::write(&oracle_path, cases).unwrap();
    let runner = root.join("jsc-runner");
    let status = Command::new("xcrun")
        .args([
            "clang",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "examples/jsc_wasm_runner.c",
            "-framework",
            "JavaScriptCore",
            "-o",
        ])
        .arg(&runner)
        .status()
        .expect("xcrun clang");
    assert!(status.success());
    // Public macOS entitlement for this standalone experiment only. Neither
    // this signature nor the presence of WebAssembly proves JIT is active.
    let allow_jit = std::env::var("WIRE_JSC_ALLOW_JIT").as_deref() == Ok("1");
    if allow_jit {
        let entitlements = root.join("allow-jit.plist");
        std::fs::write(
            &entitlements,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\
             <key>com.apple.security.cs.allow-jit</key><true/>\
             </dict></plist>\n",
        )
        .unwrap();
        let status = Command::new("codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--options",
                "runtime",
                "--entitlements",
            ])
            .arg(entitlements)
            .arg(&runner)
            .status()
            .expect("codesign");
        assert!(
            status.success(),
            "macOS experimental allow-jit signature failed"
        );
    }
    println!("JSC runner macOS allow-jit signature: {allow_jit}");
    for option in [
        "JSC_useJIT",
        "JSC_useWasmFastMemory",
        "JSC_dumpWasmDisassembly",
    ] {
        if let Ok(value) = std::env::var(option) {
            println!("diagnostic environment: {option}={value}");
        }
    }
    let result = Command::new(&runner)
        .arg(module_path)
        .arg(oracle_path)
        .output()
        .unwrap();
    let mode = if allow_jit { "allow-jit" } else { "default" };
    std::fs::write(
        root.join(format!("{name}-{mode}-report.txt")),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        root.join(format!("{name}-{mode}-stderr.txt")),
        &result.stderr,
    )
    .unwrap();
    print!("{}", String::from_utf8_lossy(&result.stdout));
    eprint!("{}", String::from_utf8_lossy(&result.stderr));
    assert!(
        result.status.success(),
        "JSC oracle/measurement runner failed"
    );
}
