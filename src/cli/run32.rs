//! PE32 launch options and user-facing execution reports.
use std::{path::Path, process::ExitCode};
use wire::{api::ApiLogger, cpu::RunOutcome};

pub(super) fn run(path: &str, opts: &[String]) -> ExitCode {
    let mut max_instr: u64 = 50_000_000;
    let mut log_dir = String::from("wire-out");
    let mut trace_cpu = false;
    let mut guest_cwd = None;
    let mut ir_path = None;
    let mut profile_opcodes = false;
    let mut present = cfg!(target_os = "macos");
    let mut software_render = false;
    let mut capture_frames = None;
    let mut stop_after_presents = None;
    let mut i = 0;
    while i < opts.len() {
        match opts[i].as_str() {
            "--max-instr" => {
                i += 1;
                max_instr = opts
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(max_instr);
            }
            "--log-dir" => {
                i += 1;
                if let Some(s) = opts.get(i) {
                    log_dir = s.clone();
                }
            }
            "--trace-cpu" => trace_cpu = true,
            "--op-stats" => profile_opcodes = true,
            "--no-op-stats" => profile_opcodes = false,
            "--headless" => present = false,
            "--present" => present = true,
            "--software-render" => software_render = true,
            "--stop-after-presents" => {
                i += 1;
                let Some(n) = opts
                    .get(i)
                    .and_then(|n| n.parse::<u64>().ok())
                    .filter(|&n| n > 0)
                else {
                    eprintln!("--stop-after-presents needs a positive count");
                    return ExitCode::from(2);
                };
                stop_after_presents = Some(n);
            }
            "--capture-frames" => {
                i += 1;
                let Some(dir) = opts.get(i) else {
                    eprintln!("--capture-frames needs a directory");
                    return ExitCode::from(2);
                };
                capture_frames = Some(std::path::PathBuf::from(dir));
            }
            "--ir" => {
                i += 1;
                let Some(path) = opts.get(i) else {
                    eprintln!("--ir needs a file");
                    return ExitCode::from(2);
                };
                ir_path = Some(path);
            }
            "--cwd" => {
                i += 1;
                let Some(dir) = opts.get(i) else {
                    eprintln!("--cwd needs a directory");
                    return ExitCode::from(2);
                };
                guest_cwd = Some(std::path::PathBuf::from(dir));
            }
            other => eprintln!("ignoring unknown option: {other}"),
        }
        i += 1;
    }
    let exe_path_buf = match Path::new(path).canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let exe_path = exe_path_buf.as_path();
    let fs = match wire::fs::GuestFs::new(
        guest_cwd
            .as_deref()
            .unwrap_or_else(|| exe_path.parent().unwrap()),
    ) {
        Ok(fs) => fs,
        Err(e) => {
            eprintln!("guest cwd: {e}");
            return ExitCode::FAILURE;
        }
    };
    let display_name = exe_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());

    std::fs::create_dir_all(&log_dir).ok();
    let api_log_path = Path::new(&log_dir).join("api.log");
    let logger = match ApiLogger::new(&api_log_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("create {}: {e}", api_log_path.display());
            return ExitCode::FAILURE;
        }
    };

    let mut process = match wire::x86::process::Process32::load(
        exe_path,
        &display_name,
        fs,
        logger,
        wire::x86::process::Config {
            present,
            capture_frames,
            gpu_enabled: !software_render,
            stop_after_presents,
        },
    ) {
        Ok(process) => process,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let loaded = &process.loaded;

    println!("loaded: {display_name}");
    println!(
        "  image base {:#010x}  entry {:#010x}  imports {}",
        loaded.image_base, loaded.entry, loaded.import_count
    );
    println!(
        "  initial eip {:#010x}  esp {:#010x}",
        loaded.entry, loaded.esp
    );
    process.logger.note(&format!(
        "run {} entry={:#x} esp={:#x}",
        display_name, loaded.entry, loaded.esp
    ));

    // Configure the owned CPU before starting the process.
    {
        let cpu = &mut process.cpu;
        let mem = &mut process.mem;
        cpu.trace_enabled = trace_cpu;
        cpu.profile_opcodes = profile_opcodes;
        if let Some(path) = ir_path {
            let result = (|| -> Result<usize, String> {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let exe = std::fs::read(exe_path).map_err(|e| e.to_string())?;
                let image = wire::ir::Image::parse(&bytes, &exe)?;
                if image.image_base != loaded.image_base {
                    return Err("IR image base does not match loaded PE".into());
                }
                cpu.install_ir(image, mem)
            })();
            match result {
            Ok(accepted) => println!("  installed IR: {accepted} instructions; unsupported/changed code uses x86 fallback"),
            Err(e) => { eprintln!("load IR: {e}"); return ExitCode::FAILURE; }
        }
        }
    }
    println!(
        "  reached entry point; starting interpretation (max {} instructions)",
        max_instr
    );
    let t_start = std::time::Instant::now();
    process.state.d3d9.run_start = Some(t_start);
    let outcome = process.run(max_instr);
    let wall = t_start.elapsed();
    let wire::x86::process::Process32 {
        cpu, state, logger, ..
    } = &mut process;

    println!("\n== run report ==");
    match &outcome {
        RunOutcome::Exited(code) => println!("outcome: exited cleanly, guest code {code}"),
        RunOutcome::InstructionLimit => println!("outcome: instruction limit hit ({max_instr})"),
        RunOutcome::PresentationLimit(n) => {
            println!("outcome: presentation benchmark limit reached ({n})")
        }
        RunOutcome::Crashed(e) => println!("outcome: CRASHED: {e}"),
        RunOutcome::CallbackReturned | RunOutcome::CallbackUnwound => {
            println!("outcome: internal error (callback frame escaped top level)")
        }
    }
    println!(
        "instructions: {} in {:.3}s ({:.2} Minstr/s)",
        cpu.instr_count,
        wall.as_secs_f64(),
        cpu.instr_count as f64 / wall.as_secs_f64().max(1e-9) / 1e6
    );
    println!("api calls: {}", logger.call_count);
    println!(
        "Metal draws: {}; GPU error: {:?}",
        state.d3d9.gpu_draws(),
        state.d3d9.gpu_error()
    );
    println!(
        "PCM: mixed_frames={}; dropped_samples={}; underrun_samples={}; locked_playback_frames={}",
        state.dsound.mixed_frames,
        state.dsound.dropped_samples,
        state.dsound.output_stats[1],
        state.dsound.locked_playback_frames
    );
    println!(
        "D3D9 draws: {}; presents: {}; displayed: {}",
        state.d3d9.draws, state.d3d9.frames, state.presenter.displayed
    );
    if let Some(e) = &state.presenter.error {
        eprintln!("presenter: {e}");
    }
    println!(
        "first D3D9 draw: {:?}s; first present: {:?}s",
        state.d3d9.first_draw_seconds, state.d3d9.first_present_seconds
    );
    if ir_path.is_some() {
        println!(
            "IR instructions: {}; x86 fallback: {}; CALL_NATIVE: {}",
            cpu.ir_steps, cpu.ir_fallbacks, cpu.ir_native_calls
        );
        println!(
            "static scalar runs: {}; guest instructions in runs: {}",
            cpu.ir_scalar_runs, cpu.ir_scalar_steps
        );
    }
    println!(
        "final regs: eax={:#010x} ebx={:#010x} ecx={:#010x} edx={:#010x}",
        cpu.regs[0], cpu.regs[3], cpu.regs[1], cpu.regs[2]
    );
    println!(
        "            esp={:#010x} ebp={:#010x} esi={:#010x} edi={:#010x}",
        cpu.regs[4], cpu.regs[5], cpu.regs[6], cpu.regs[7]
    );
    println!(
        "            eip={:#010x} eflags={:#06x}",
        cpu.eip, cpu.eflags
    );

    if !logger.unimpl_freq.is_empty() {
        println!("\ntop unimplemented APIs:");
        let mut v: Vec<(&String, &u64)> = logger.unimpl_freq.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        for (k, c) in v.iter().take(15) {
            println!("  {c:>6}  {k}");
        }
    }
    if cpu.profile_opcodes {
        println!("\ntop executed opcodes ({} distinct):", cpu.op_stats.len());
        let mut v: Vec<(&u32, &u64)> = cpu.op_stats.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        // Include the secondary 0F opcodes, whose aggregate otherwise hides
        // the actual instruction mix below the first few scalar operations.
        for (k, c) in v.iter().take(40) {
            let name = if **k >= 0x100 {
                format!("0f {:02x}", **k & 0xff)
            } else {
                format!("{:02x}", **k)
            };
            println!("  {c:>9}  {name}");
        }
        println!("\ntop raw fallback opcodes:");
        let mut v: Vec<_> = cpu.raw_op_stats.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        for (k, c) in v.iter().take(40) {
            if **k >= 0x100 {
                println!("  {c:>9}  0f {:02x}", **k & 0xff);
            } else {
                println!("  {c:>9}  {k:02x}");
            }
        }
        println!(
            "\ncompact IR operations: {}",
            cpu.compact_op_stats.iter().sum::<u64>()
        );
        println!("top cold IR operations (sub-operation, width, operands):");
        let mut v = cpu.cold_ir_profile();
        v.sort_by_key(|a| std::cmp::Reverse(a.1));
        for (name, count) in v.iter().take(30) {
            println!("  {count:>9}  {name}");
        }
    }
    if matches!(outcome, RunOutcome::Crashed(_)) {
        println!("\nlast API calls before stop:");
        for l in logger
            .recent
            .iter()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .iter()
            .rev()
        {
            println!("  {l}");
        }
        if !cpu.trace_ring.is_empty() {
            println!(
                "\nlast {} executed instructions (eip: bytes):",
                cpu.trace_ring.len().min(32)
            );
            for (eip, bytes) in cpu
                .trace_ring
                .iter()
                .rev()
                .take(32)
                .collect::<Vec<_>>()
                .iter()
                .rev()
            {
                let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
                println!("  {eip:#010x}: {}", hex.join(" "));
            }
        }
    }
    logger.finish();
    println!("\nfull API trace: {}", api_log_path.display());

    match outcome {
        RunOutcome::Exited(0) => ExitCode::SUCCESS,
        RunOutcome::PresentationLimit(_) => ExitCode::SUCCESS,
        RunOutcome::Exited(c) => ExitCode::from((c & 0xff) as u8),
        _ => ExitCode::FAILURE,
    }
}
