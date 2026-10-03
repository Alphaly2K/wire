use std::path::Path;
use std::process::ExitCode;
use wire::api::{ApiLogger, ApiState, ApiTable};
use wire::cpu::{Cpu, RunOutcome};
use wire::loader;
use wire::mem::GuestMem;
use wire::pe::PeFile;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("wire — Watari-Is-a-Runtime-Environment (M0 spike)");
        eprintln!("usage:");
        eprintln!("  wire imports <program.exe>");
        eprintln!("  wire run <program.exe> [--max-instr N] [--log-dir DIR]");
        return ExitCode::from(2);
    }
    match args[1].as_str() {
        "imports" => cmd_imports(&args[2]),
        "run" => cmd_run(&args[2], &args[3..]),
        c => {
            eprintln!("unknown command: {c}");
            ExitCode::from(2)
        }
    }
}

fn cmd_imports(path: &str) -> ExitCode {
    let buf = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pe = match PeFile::parse(&buf) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parse {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("file:        {path}");
    println!("image base:  {:#010x}", pe.image_base);
    println!("entry rva:   {:#010x}", pe.entry_rva);
    println!("subsystem:   {}", pe.subsystem);
    println!("image size:  {:#x}", pe.size_of_image);
    println!(
        "stack:       reserve {:#x} commit {:#x}",
        pe.stack_reserve, pe.stack_commit
    );
    println!("sections:");
    for s in &pe.sections {
        let mut flags = String::new();
        if s.is_executable() {
            flags.push('X');
        }
        if s.is_writable() {
            flags.push('W');
        }
        let entry_mark = if s.contains_rva(pe.entry_rva) { "  <== entry" } else { "" };
        println!(
            "  {:8} va={:#010x} vsize={:#09x} raw={:#09x} {}{}",
            s.name, s.virtual_address, s.virtual_size, s.raw_size, flags, entry_mark
        );
    }
    match pe.imports(&buf) {
        Ok(imports) => {
            let total: usize = imports.iter().map(|d| d.functions.len()).sum();
            println!("\nimports: {} DLLs, {} functions", imports.len(), total);
            for d in &imports {
                println!("\n  {} ({}):", d.dll, d.functions.len());
                let mut line = String::from("    ");
                for f in &d.functions {
                    let n = f
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("#{}", f.ordinal.unwrap_or(0)));
                    if line.len() + n.len() > 100 {
                        println!("{line}");
                        line = String::from("    ");
                    }
                    line.push_str(&n);
                    line.push(' ');
                }
                if line.trim().len() > 0 {
                    println!("{line}");
                }
            }
            // rough packing heuristics
            println!("\nheuristics:");
            let entry_sec = pe.entry_section();
            match entry_sec {
                Some(s) => {
                    println!("  entry section: {}", s.name);
                    if s.is_writable() {
                        println!("  WARNING: entry section is writable (common in packed binaries)");
                    }
                }
                None => println!("  WARNING: entry point not inside any section"),
            }
            if total < 20 {
                println!("  WARNING: very few imports ({total}) — possible packer");
            }
        }
        Err(e) => {
            eprintln!("import walk: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn cmd_run(path: &str, opts: &[String]) -> ExitCode {
    let mut max_instr: u64 = 50_000_000;
    let mut log_dir = String::from("wire-out");
    let mut trace_cpu = false;
    let mut i = 0;
    while i < opts.len() {
        match opts[i].as_str() {
            "--max-instr" => {
                i += 1;
                max_instr = opts.get(i).and_then(|s| s.parse().ok()).unwrap_or(max_instr);
            }
            "--log-dir" => {
                i += 1;
                if let Some(s) = opts.get(i) {
                    log_dir = s.clone();
                }
            }
            "--trace-cpu" => trace_cpu = true,
            other => eprintln!("ignoring unknown option: {other}"),
        }
        i += 1;
    }
    let exe_path = Path::new(path);
    let display_name = exe_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());

    std::fs::create_dir_all(&log_dir).ok();
    let api_log_path = Path::new(&log_dir).join("api.log");
    let mut logger = match ApiLogger::new(&api_log_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("create {}: {e}", api_log_path.display());
            return ExitCode::FAILURE;
        }
    };

    let mut mem = match GuestMem::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("guest memory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut api = ApiTable::new();
    let loaded = match loader::load_image(&mut mem, &mut api, exe_path, &display_name) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("load {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut state = ApiState::new(
        loaded.image_base,
        path.to_string(),
        loaded.cmdline_a,
        loaded.cmdline_w,
    );

    println!("loaded: {display_name}");
    println!(
        "  image base {:#010x}  entry {:#010x}  imports {}",
        loaded.image_base, loaded.entry, loaded.import_count
    );
    println!("  initial eip {:#010x}  esp {:#010x}", loaded.entry, loaded.esp);
    logger.note(&format!(
        "run {} entry={:#x} esp={:#x}",
        display_name, loaded.entry, loaded.esp
    ));

    // --- execution reaches the entry point here ---
    let mut cpu = Cpu::new(loaded.entry, loaded.esp, loaded.teb);
    cpu.trace_enabled = trace_cpu;
    println!(
        "  reached entry point; starting interpretation (max {} instructions)",
        max_instr
    );
    let t_start = std::time::Instant::now();
    let outcome = cpu.run(&mut mem, &mut api, &mut state, &mut logger, max_instr);
    let wall = t_start.elapsed();

    println!("\n== run report ==");
    match &outcome {
        RunOutcome::Exited(code) => println!("outcome: exited cleanly, guest code {code}"),
        RunOutcome::InstructionLimit => println!("outcome: instruction limit hit ({max_instr})"),
        RunOutcome::Crashed(e) => println!("outcome: CRASHED: {e}"),
        RunOutcome::CallbackReturned => println!("outcome: internal error (callback frame escaped top level)"),
    }
    println!(
        "instructions: {} in {:.3}s ({:.2} Minstr/s)",
        cpu.instr_count,
        wall.as_secs_f64(),
        cpu.instr_count as f64 / wall.as_secs_f64().max(1e-9) / 1e6
    );
    println!("api calls: {}", logger.call_count);
    println!(
        "final regs: eax={:#010x} ebx={:#010x} ecx={:#010x} edx={:#010x}",
        cpu.regs[0], cpu.regs[3], cpu.regs[1], cpu.regs[2]
    );
    println!(
        "            esp={:#010x} ebp={:#010x} esi={:#010x} edi={:#010x}",
        cpu.regs[4], cpu.regs[5], cpu.regs[6], cpu.regs[7]
    );
    println!("            eip={:#010x} eflags={:#06x}", cpu.eip, cpu.eflags);

    if !logger.unimpl_freq.is_empty() {
        println!("\ntop unimplemented APIs:");
        let mut v: Vec<(&String, &u64)> = logger.unimpl_freq.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        for (k, c) in v.iter().take(15) {
            println!("  {c:>6}  {k}");
        }
    }
    {
        println!("\ntop executed opcodes ({} distinct):", cpu.op_stats.len());
        let mut v: Vec<(&u32, &u64)> = cpu.op_stats.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        for (k, c) in v.iter().take(15) {
            let name = if **k >= 0x100 {
                format!("0f {:02x}", **k & 0xff)
            } else {
                format!("{:02x}", **k)
            };
            println!("  {c:>9}  {name}");
        }
    }
    if matches!(outcome, RunOutcome::Crashed(_)) {
        println!("\nlast API calls before stop:");
        for l in logger.recent.iter().rev().take(12).collect::<Vec<_>>().iter().rev() {
            println!("  {l}");
        }
        if !cpu.trace_ring.is_empty() {
            println!("\nlast {} executed instructions (eip: bytes):", cpu.trace_ring.len().min(32));
            for (eip, bytes) in cpu.trace_ring.iter().rev().take(32).collect::<Vec<_>>().iter().rev() {
                let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
                println!("  {eip:#010x}: {}", hex.join(" "));
            }
        }
    }
    logger.finish();
    println!("\nfull API trace: {}", api_log_path.display());

    match outcome {
        RunOutcome::Exited(0) => ExitCode::SUCCESS,
        RunOutcome::Exited(c) => ExitCode::from((c & 0xff) as u8),
        _ => ExitCode::FAILURE,
    }
}
