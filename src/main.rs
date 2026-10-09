use std::path::Path;
use std::process::ExitCode;
use wire::api::{ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::{Cpu, RunOutcome};
use wire::loader;
use wire::mem::GuestMem;
use wire::pe::PeFile;
use wire::thread::Sched;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("wire — Watari-Is-a-Runtime-Environment (M2 spike)");
        eprintln!("usage:");
        eprintln!("  wire imports <program.exe>");
        eprintln!("  wire probe64 <program.exe> [--max-instr N] [--startup] [--dll] [--window] [--cpu-stats] [--api-trace PATH] [--guest-arg VALUE] [--graphics-backend PATH --moltenvk PATH] (experimental AMD64 diagnostic)");
        eprintln!("  wire prepare <program.exe> [--out program.wir]");
        eprintln!(
            "  wire run <program.exe> [--ir program.wir] [--op-stats] [--max-instr N] [--log-dir DIR] [--cwd DIR] [--trace-cpu] [--present|--headless] [--software-render] [--capture-frames DIR] [--stop-after-presents N]"
        );
        return ExitCode::from(2);
    }
    match args[1].as_str() {
        "imports" => cmd_imports(&args[2]),
        "probe64" => cmd_probe64(&args[2], &args[3..]),
        "prepare" => cmd_prepare(&args[2], &args[3..]),
        "run" => cmd_run(&args[2], &args[3..]),
        c => {
            eprintln!("unknown command: {c}");
            ExitCode::from(2)
        }
    }
}

fn cmd_prepare(path: &str, opts: &[String]) -> ExitCode {
    let default = Path::new("wire-out").join(format!(
        "{}.wir",
        Path::new(path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    let out = match opts {
        [] => default,
        [flag, value] if flag == "--out" => value.into(),
        _ => {
            eprintln!("usage: wire prepare <program.exe> [--out program.wir]");
            return ExitCode::from(2);
        }
    };
    let result = (|| -> Result<(), String> {
        let exe = std::fs::read(path).map_err(|e| e.to_string())?;
        let start = std::time::Instant::now();
        let image = wire::ir::Image::from_pe(&exe)?;
        let bytes = image.encode();
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&out, &bytes).map_err(|e| e.to_string())?;
        println!("prepared: {}", out.display());
        println!(
            "  {} fixed {}-byte records; {} lifted ({} compact); {} flag writers partially deferred; {} native call sites",
            image.records.len(),
            wire::ir::RECORD_BYTES,
            image.lifted_count(),
            image.compact_count(),
            image.elided_flag_writers(),
            image.native_call_count()
        );
        println!(
            "  {} bytes in {:.3}s; no host machine code generated",
            bytes.len(),
            start.elapsed().as_secs_f64()
        );
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("prepare: {e}");
            ExitCode::FAILURE
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
            if let Ok(pe) = wire::x64::pe::Pe64::parse(&buf) {
                println!(
                    "file: {path}\ntarget: AMD64 / PE32+\nimage base: {:#x}\nentry rva: {:#x}",
                    pe.image_base, pe.entry_rva
                );
                return match pe.imports(&buf) {
                    Ok(imports) => {
                        for d in imports {
                            println!("{}:", d.dll);
                            for f in d.functions {
                                println!(
                                    "  {} IAT={:#x}",
                                    f.name
                                        .unwrap_or_else(|| format!("#{}", f.ordinal.unwrap_or(0))),
                                    f.iat_rva
                                );
                            }
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("imports: {e}");
                        ExitCode::FAILURE
                    }
                };
            }
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
        let entry_mark = if s.contains_rva(pe.entry_rva) {
            "  <== entry"
        } else {
            ""
        };
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
                if !line.trim().is_empty() {
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
                        println!(
                            "  WARNING: entry section is writable (common in packed binaries)"
                        );
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
        format!("C:\\{display_name}"),
        loaded.cmdline_a,
        loaded.cmdline_w,
    );
    state.fs = Some(fs);
    state.stop_after_presents = stop_after_presents;
    state.presenter = match wire::platform::Presenter::new(present, capture_frames) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("presenter: {e}");
            return ExitCode::FAILURE;
        }
    };
    state.d3d9.enable_gpu = !software_render;
    state.d3d9.prepare_gpu();
    if let Err(e) = state.modules.register_main(
        &mem,
        loaded.image_base,
        Path::new(exe_path)
            .canonicalize()
            .unwrap_or_else(|_| Path::new(exe_path).to_path_buf()),
        state.exe_path.clone(),
    ) {
        eprintln!("register main module: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) =
        state
            .modules
            .setup_tls_thread(&mut mem, &mut state.heap, state.process_heap, loaded.teb)
    {
        eprintln!("initialize main static TLS: {e}");
        return ExitCode::FAILURE;
    }
    for dll in api.imported_dlls() {
        if wire::modules::native_module(dll) {
            state.module_handle_for(dll);
        }
    }

    println!("loaded: {display_name}");
    println!(
        "  image base {:#010x}  entry {:#010x}  imports {}",
        loaded.image_base, loaded.entry, loaded.import_count
    );
    println!(
        "  initial eip {:#010x}  esp {:#010x}",
        loaded.entry, loaded.esp
    );
    logger.note(&format!(
        "run {} entry={:#x} esp={:#x}",
        display_name, loaded.entry, loaded.esp
    ));

    // --- execution reaches the entry point here ---
    let mut cpu = Cpu::new(loaded.entry, loaded.esp, loaded.teb);
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
            cpu.install_ir(image, &mem)
        })();
        match result {
            Ok(accepted) => println!("  installed IR: {accepted} instructions; unsupported/changed code uses x86 fallback"),
            Err(e) => { eprintln!("load IR: {e}"); return ExitCode::FAILURE; }
        }
    }
    let mut sched = Sched::new();
    let main_tid = sched.alloc_tid();
    let main_handle = state.alloc_handle(HandleObj::Thread { tid: main_tid });
    sched.add_main(main_tid, main_handle, loaded.teb);
    println!(
        "  reached entry point; starting interpretation (max {} instructions)",
        max_instr
    );
    let t_start = std::time::Instant::now();
    state.d3d9.run_start = Some(t_start);
    let outcome = cpu.run(
        &mut mem,
        &mut api,
        &mut state,
        &mut logger,
        &mut sched,
        max_instr,
    );
    let wall = t_start.elapsed();

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

fn cmd_probe64(path: &str, opts: &[String]) -> ExitCode {
    let result = (|| -> Result<(), String> {
        let mut limit = 1000000;
        let mut startup = false;
        let mut dll_attach = false;
        let mut window = false;
        let mut cpu_stats = false;
        let mut cpu_diagnostics = false;
        let mut cache_layout = wire::x64::memory::TranslationCacheLayout::default();
        let mut api_trace = None;
        let mut capture_frame = None;
        let mut graphics_backend = None;
        let mut moltenvk = None;
        let mut guest_args = Vec::new();
        let mut opt = opts.iter();
        while let Some(flag) = opt.next() {
            match flag.as_str() {
                "--guest-arg" => {
                    guest_args.push(opt.next().ok_or("--guest-arg needs a value")?.clone());
                    startup = true;
                }
                "--graphics-backend" => {
                    graphics_backend = Some(opt.next().ok_or("--graphics-backend needs a path")?);
                    startup = true;
                }
                "--moltenvk" => {
                    moltenvk = Some(opt.next().ok_or("--moltenvk needs a path")?);
                    startup = true;
                }
                "--startup" => startup = true,
                "--cpu-stats" => cpu_stats = true,
                "--cpu-diagnostics" => {
                    cpu_stats = true;
                    cpu_diagnostics = true;
                }
                "--translation-cache" => {
                    use wire::x64::memory::TranslationCacheLayout as L;
                    cache_layout = match opt.next().map(String::as_str) {
                        Some("direct16k") => L::Direct16K,
                        Some("direct32k") => L::Direct32K,
                        Some("direct64k") => L::Direct64K,
                        Some("direct128k") => L::Direct128K,
                        Some("fourway16k") => L::FourWay16K,
                        _ => return Err("--translation-cache needs direct16k, direct32k, direct64k, direct128k or fourway16k".into()),
                    };
                }
                "--api-trace" => {
                    api_trace = Some(opt.next().ok_or("--api-trace needs a path")?.as_str());
                    startup = true;
                }
                "--capture-frame" => {
                    capture_frame = Some(opt.next().ok_or("--capture-frame needs a PPM path")?.as_str());
                    window = true;
                    startup = true;
                }
                "--window" => {
                    window = true;
                    startup = true;
                }
                "--dll" => {
                    dll_attach = true;
                    startup = true;
                }
                "--max-instr" => {
                    limit = opt
                        .next()
                        .ok_or("--max-instr needs a count")?
                        .parse::<u64>()
                        .map_err(|e| e.to_string())?
                }
                _ => {
                    return Err(
                        "usage: wire probe64 <exe> [--max-instr N] [--startup] [--dll] [--window] [--cpu-stats] [--api-trace PATH] [--capture-frame PPM] [--guest-arg VALUE] [--graphics-backend PATH --moltenvk PATH]"
                            .into(),
                    )
                }
            }
        }
        let graphics = match (graphics_backend, moltenvk) {
            (Some(backend), Some(moltenvk)) => Some(wire::gpu::NativeGraphics::load(
                Path::new(backend),
                Path::new(moltenvk),
            )?),
            (None, None) => None,
            _ => return Err("--graphics-backend and --moltenvk must be provided together".into()),
        };
        if startup {
            return run_startup64(
                path,
                limit,
                dll_attach,
                window,
                cpu_stats,
                cpu_diagnostics,
                cache_layout,
                graphics,
                api_trace,
                capture_frame,
                &guest_args,
            );
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let pe = wire::x64::pe::Pe64::parse(&bytes)?;
        let mut gateways = std::collections::BTreeMap::new();
        let mut mem = pe.load(&bytes, pe.image_base, |dll, f| {
            let target = 0x7ffe00000000 + gateways.len() as u64 * 16;
            gateways.insert(
                target,
                format!(
                    "{dll}!{}",
                    f.name
                        .as_deref()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("#{}", f.ordinal.unwrap_or(0)))
                ),
            );
            Ok(target)
        })?;
        const STACK: u64 = 0x7ff000000000;
        const RETURN: u64 = 0x7ffe10000000;
        let reserve = pe
            .stack_reserve
            .max(4096)
            .checked_add(4095)
            .ok_or("stack reserve overflow")?
            / 4096
            * 4096;
        if reserve > 64 * 1024 * 1024 {
            return Err("probe64 stack exceeds 64 MiB".into());
        }
        mem.map(
            STACK,
            reserve,
            wire::x64::memory::READ | wire::x64::memory::WRITE,
        )?;
        let rsp = STACK + reserve - 40;
        mem.write_int(rsp, 8, RETURN)?;
        if cpu_stats {
            mem.enable_execution_stats();
        }
        mem.set_translation_cache_layout(cache_layout)?;
        if cpu_diagnostics {
            mem.enable_execution_diagnostics();
            mem.register_code_image(pe.image_base, path.to_string());
        }
        let mut cpu = wire::x64::cpu::Cpu64::new(pe.image_base + pe.entry_rva as u64, rsp);
        println!("experimental AMD64 CPU probe: base={:#x} RIP={:#x} RSP={rsp:#x}; startup services disabled", pe.image_base, cpu.rip);
        let start = std::time::Instant::now();
        while cpu.work < limit {
            if cpu.rip == RETURN {
                report_execution64(start, &cpu, &mem);
                println!(
                    "entry returned RAX={:#x}; retired={}",
                    cpu.regs[0], cpu.retired
                );
                return Ok(());
            }
            if let Some(name) = gateways.get(&cpu.rip) {
                report_execution64(start, &cpu, &mem);
                println!(
                    "import boundary: {name}; retired={}; integer args={:x?}",
                    cpu.retired,
                    cpu.arguments(&mem, 4)?
                );
                return Err(
                    "AMD64 import is unresolved; this diagnostic does not run the game".into(),
                );
            }
            if let Err(e) =
                cpu.step_with_budget_guarded(&mut mem, limit - cpu.work, |start, end| {
                    gateways
                        .range((
                            std::ops::Bound::Included(start),
                            std::ops::Bound::Excluded(end),
                        ))
                        .next()
                        .is_none()
                        && !(RETURN >= start && RETURN < end)
                })
            {
                report_execution64(start, &cpu, &mem);
                println!(
                    "retired={}; RIP={:#x}; regs={:x?}",
                    cpu.retired, cpu.rip, cpu.regs
                );
                return Err(e);
            }
        }
        report_execution64(start, &cpu, &mem);
        Err(format!(
            "AMD64 probe budget limit ({limit}, includes REP elements) at {:#x}",
            cpu.rip
        ))
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("probe64: {e}");
            ExitCode::FAILURE
        }
    }
}

fn report_execution64(
    start: std::time::Instant,
    cpu: &wire::x64::cpu::Cpu64,
    mem: &wire::x64::memory::Memory,
) {
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "AMD64 execution: retired={} work={} seconds={elapsed:.6} MIPS={:.3}",
        cpu.retired,
        cpu.work,
        cpu.retired as f64 / elapsed.max(1e-9) / 1e6
    );
    if let Some(stats) = mem.execution_stats() {
        println!(
            "AMD64 CPU stats: IR={} fallback={} translations={} invalidations={}",
            stats.ir_steps, stats.fallback_steps, stats.translations, stats.invalidations
        );
    }
    if let Some(report) = mem.execution_diagnostics_report() {
        print!("{report}");
    }
}

#[allow(clippy::too_many_arguments)]
fn run_startup64(
    path: &str,
    limit: u64,
    dll_attach: bool,
    window: bool,
    cpu_stats: bool,
    cpu_diagnostics: bool,
    cache_layout: wire::x64::memory::TranslationCacheLayout,
    graphics: Option<wire::gpu::NativeGraphics>,
    api_trace: Option<&str>,
    capture_frame: Option<&str>,
    guest_args: &[String],
) -> Result<(), String> {
    use wire::x64::process::{Outcome, Process64};
    let mut process = Process64::load(Path::new(path), dll_attach)?;
    process
        .runtime
        .append_guest_arguments(&mut process.memory, guest_args)?;
    if api_trace.is_some() {
        process.runtime.enable_api_inventory();
    }
    if let Some(graphics) = graphics {
        let info = graphics.adapter();
        println!(
            "native Vulkan adapter: {} vendor={:#x} device={:#x}; experimental D3D11 bridge",
            info.name, info.vendor, info.device
        );
        process.runtime.attach_graphics(graphics)?;
    }
    if window {
        process.enable_window()?;
    }
    if let Some(path) = capture_frame {
        process.capture_frame_to(Path::new(path))?;
        println!("diagnostic Present capture: {path}; GPU readback enabled, timing is not a performance comparison");
    }
    println!(
        "experimental AMD64 startup: RIP={:#x} RSP={:#x}; interpreter; DLL attach={dll_attach}",
        process.cpu.rip, process.cpu.regs[4]
    );
    for (name, base) in &process.modules {
        println!("  mapped {name} at {base:#x}");
    }
    if cpu_stats {
        process.memory.enable_execution_stats();
    }
    process.memory.set_translation_cache_layout(cache_layout)?;
    if cpu_diagnostics {
        process.memory.enable_execution_diagnostics();
    }
    let start = std::time::Instant::now();
    let outcome = loop {
        let slice = process.cpu.work.saturating_add(25_000_000).min(limit);
        let result = process.run(slice);
        if matches!(result, Ok(Outcome::Budget)) && process.cpu.work < limit {
            let memory = process.memory.memory_status();
            let committed = u64::from_le_bytes(memory[8..16].try_into().unwrap())
                - u64::from_le_bytes(memory[16..24].try_into().unwrap());
            println!(
                "startup progress: work={} retired={} RIP={:#x} API calls={} committedMiB={} phase={}",
                process.cpu.work,
                process.cpu.retired,
                process.cpu.rip,
                process.runtime.api_calls,
                committed / 1048576,
                process.phase()
            );
            for line in process.runtime.trace.iter().rev().take(3).rev() {
                println!("  {line}");
            }
        } else {
            break result;
        }
    };
    report_execution64(start, &process.cpu, &process.memory);
    if let Some(path) = api_trace {
        std::fs::write(
            path,
            process
                .runtime
                .api_inventory_tsv()
                .ok_or("missing API inventory")?,
        )
        .map_err(|e| format!("write Win64 API inventory: {e}"))?;
        println!("Win64 aggregate API inventory: {path}");
    }
    process.poll_window(true)?;
    if outcome.is_err() {
        println!(
            "fault ABI registers: RCX={:#x} RDX={:#x} R8={:#x} R9={:#x}; thread GS={:#x}",
            process.cpu.regs[1],
            process.cpu.regs[2],
            process.cpu.regs[8],
            process.cpu.regs[9],
            process.cpu.gs_base
        );
    }
    for (name, pc) in &process.stages {
        if name.contains(" TLS attach") {
            println!("TLS process attach: {pc:#x} ({name})");
        } else {
            println!("entry stage: {name} at {pc:#x}");
        }
    }
    for line in &process.runtime.trace {
        println!("  {line}");
    }
    println!(
        "phase={}; retired={}; budget work={}; RIP={:#x}; API calls={}",
        process.phase(),
        process.cpu.retired,
        process.cpu.work,
        process.cpu.rip,
        process.runtime.api_calls
    );
    for w in process.runtime.window_info() {
        println!(
            "Win64 guest window: HWND={:#x} visible={} client={}x{} title={:?}",
            w.handle, w.visible, w.width, w.height, w.title
        );
    }
    println!(
        "Win64 native GPU draws: {}; presents: {}",
        process.runtime.graphics_draws(),
        process.runtime.graphics_presents()
    );
    match outcome? {
        Outcome::Returned(value) => {
            println!(
                "entry returned RAX={value:#x}; retired={}",
                process.cpu.retired
            );
            Ok(())
        }
        Outcome::Exited(0) => {
            println!("AMD64 process exited 0");
            Ok(())
        }
        Outcome::Exited(code) => Err(format!("AMD64 process exited {code}")),
        Outcome::Budget => Err(format!(
            "AMD64 startup budget limit ({limit}, includes REP elements, native calls and idle scheduler polls)"
        )),
    }
}
