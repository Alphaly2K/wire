//! Experimental AMD64 probe/startup command and its execution reports.
use std::{path::Path, process::ExitCode};

pub(super) fn probe(path: &str, opts: &[String]) -> ExitCode {
    let result = (|| -> Result<(), String> {
        let mut limit = 1000000;
        let mut startup = false;
        let mut dll_attach = false;
        let mut window = false;
        let mut cpu_stats = false;
        let mut cpu_diagnostics = false;
        let mut cache_layout = wire::x64::memory::TranslationCacheLayout::default();
        let mut api_trace = None;
        let mut mono_trace = None;
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
                "--mono-trace" => {
                    mono_trace = Some(opt.next().ok_or("--mono-trace needs a path")?.as_str());
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
                        "usage: wire probe64 <exe> [--max-instr N] [--startup] [--dll] [--window] [--cpu-stats] [--api-trace PATH] [--mono-trace PATH] [--capture-frame PPM] [--guest-arg VALUE] [--graphics-backend PATH --moltenvk PATH]"
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
                mono_trace,
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
    mono_trace: Option<&str>,
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
    if mono_trace.is_some() {
        process.enable_mono_trace()?;
        println!(
            "diagnostic original Mono entry trace enabled; timing is not a performance comparison"
        );
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
    if let Some(path) = mono_trace {
        std::fs::write(
            path,
            process.mono_trace_tsv().ok_or("missing Mono entry trace")?,
        )
        .map_err(|e| format!("write original Mono entry trace: {e}"))?;
        println!("original Mono entry observations: {path}");
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
