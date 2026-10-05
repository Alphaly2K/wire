//! Repeatable guest-instruction throughput comparison (no API/trace/profiling).
use std::time::Instant;
use wire::api::{ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::{Cpu, RunOutcome};
use wire::ir::Image;
use wire::mem::GuestMem;
use wire::thread::Sched;
fn main() {
    let iterations = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(3_000_000);
    let mut code = vec![0xb9];
    code.extend(iterations.to_le_bytes());
    code.extend([0xba, 1, 0, 0, 0, 0x31, 0xc0]);
    // Six instructions per iteration: redundant flags, one live carry chain,
    // DEC/JNZ. End on INT3 so both engines stop at exactly the same boundary.
    code.extend([
        0x01, 0xd0, 0x83, 0xc2, 1, 0x31, 0xc3, 0x39, 0xd0, 0x49, 0x75, 0xf4, 0xcc,
    ]);
    let image = Image::from_code(0x10000, &code).unwrap();
    let bytes = image.encode();
    println!(
        "{} guest instructions, {} IR bytes, {} flag writers deferred",
        image.records.len(),
        bytes.len(),
        image.elided_flag_writers()
    );
    let mut reference = None;
    for round in 0..6 {
        let ir = round % 2 == 1;
        let mut mem = GuestMem::new().unwrap();
        mem.commit(0x10000, 0x10000).unwrap();
        mem.commit(0x30000, 0x10000).unwrap();
        mem.write_bytes(0x10000, &code).unwrap();
        let mut cpu = Cpu::new(0x10000, 0x3fff0, 0);
        cpu.profile_opcodes = false;
        if ir {
            cpu.install_ir(Image::parse(&bytes, &code).unwrap(), &mem)
                .unwrap();
        }
        let mut api = ApiTable::new();
        let mut state = ApiState::new(0x10000, "C:\\bench.exe".into(), 0, 0);
        let path = std::env::temp_dir().join(format!("wire-ir-bench-{}.log", std::process::id()));
        let mut log = ApiLogger::new(&path).unwrap();
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
            iterations as u64 * 8 + 100,
        );
        let elapsed = start.elapsed().as_secs_f64();
        assert!(matches!(outcome,RunOutcome::Crashed(ref s) if s.contains("breakpoint")));
        let result = (cpu.regs, cpu.eflags, cpu.eip, cpu.instr_count);
        if let Some(expected) = reference {
            assert_eq!(result, expected);
        } else {
            reference = Some(result);
        }
        println!(
            "{} {:.2} guest MIPS ({:.3}s, {} instructions, {} IR / {} fallback)",
            if ir { "IR " } else { "x86" },
            cpu.instr_count as f64 / elapsed / 1e6,
            elapsed,
            cpu.instr_count,
            cpu.ir_steps,
            cpu.ir_fallbacks
        );
        drop(log);
        std::fs::remove_file(path).unwrap();
    }
}
