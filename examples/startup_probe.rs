//! Resume a real PE32 startup after optionally clicking a dialog button.
//! Uses queued guest input; never patches executable bytes or guest branches.
//! startup_probe EXE IR LOG_DIR [BUTTON_ID] [INSTRUCTION_LIMIT] [input] [PRESENT_LIMIT] [visible] [enter]
use wire::{
    api::{ApiLogger, ApiState, ApiTable, HandleObj},
    cpu::{Cpu, RunOutcome},
    fs::GuestFs,
    loader,
    mem::GuestMem,
    thread::Sched,
};
fn outcome_text(outcome: &RunOutcome) -> String {
    match outcome {
        RunOutcome::Exited(n) => format!("exit {n:#x}"),
        RunOutcome::InstructionLimit => "instruction limit".into(),
        RunOutcome::Crashed(e) => format!("crash: {e}"),
        RunOutcome::PresentationLimit(n) => format!("present limit {n}"),
        _ => "callback boundary".into(),
    }
}
fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        return Err("EXE IR LOG_DIR [BUTTON_ID] [INSTRUCTION_LIMIT] required".into());
    }
    let path = std::path::Path::new(&args[0])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let log_dir = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
    let mut mem = GuestMem::new().map_err(|e| e.to_string())?;
    let mut api = ApiTable::new();
    let display = path.file_name().unwrap().to_string_lossy();
    let loaded = loader::load_image(&mut mem, &mut api, &path, &display)?;
    let mut state = ApiState::new(
        loaded.image_base,
        format!("C:\\{display}"),
        loaded.cmdline_a,
        loaded.cmdline_w,
    );
    state.fs = Some(GuestFs::new(path.parent().unwrap()).map_err(|e| e.to_string())?);
    state.modules.register_main(
        &mem,
        loaded.image_base,
        path.clone(),
        state.exe_path.clone(),
    )?;
    state
        .modules
        .setup_tls_thread(&mut mem, &mut state.heap, state.process_heap, loaded.teb)?;
    for dll in api.imported_dlls() {
        if wire::modules::native_module(dll) {
            state.module_handle_for(dll);
        }
    }
    state.presenter = wire::platform::Presenter::new(
        args.get(7).is_some_and(|s| s == "visible"),
        Some(log_dir.join("frames")),
    )?;
    state.d3d9.enable_gpu = !args.iter().any(|s| s == "software");
    state.d3d9.prepare_gpu();
    state.stop_after_presents = Some(
        args.get(6)
            .map(|s| s.parse::<u64>().map_err(|e| e.to_string()))
            .transpose()?
            .unwrap_or(3),
    );
    let mut log = ApiLogger::new(&log_dir.join("api.log")).map_err(|e| e.to_string())?;
    let mut cpu = Cpu::new(loaded.entry, loaded.esp, loaded.teb);
    // Match normal `wire run`: per-opcode profiling disables IR chaining and
    // adds hash-table work to raw execution, distorting streaming deadlines.
    cpu.profile_opcodes = false;
    let ir = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let exe = std::fs::read(&path).map_err(|e| e.to_string())?;
    cpu.install_ir(wire::ir::Image::parse(&ir, &exe)?, &mem)?;
    let mut sched = Sched::new();
    let tid = sched.alloc_tid();
    let handle = state.alloc_handle(HandleObj::Thread { tid });
    sched.add_main(tid, handle, loaded.teb);
    let button = args
        .get(3)
        .map(|s| s.parse::<u32>().map_err(|e| e.to_string()))
        .transpose()?
        .filter(|id| *id != 0);
    let limit = args
        .get(4)
        .map(|s| s.parse::<u64>().map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or(1000000000);
    let outcome = cpu.run(&mut mem, &mut api, &mut state, &mut log, &mut sched, 500000);
    println!("initial: {}", outcome_text(&outcome));
    for (&hwnd, w) in &state.user32.windows {
        println!(
            "window {hwnd:#x}: {} parent={:#x} id={} rect={:?}",
            state.user32.classes[&w.class].name, w.parent, w.menu, w.rect
        );
    }
    if matches!(outcome, RunOutcome::InstructionLimit) {
        if let Some(id) = button {
            let (&hwnd, w) = state
                .user32
                .windows
                .iter()
                .find(|(_, w)| {
                    w.parent != 0
                        && w.menu == id
                        && state.user32.classes[&w.class]
                            .name
                            .eq_ignore_ascii_case("Button")
                })
                .ok_or("requested Button ID not present")?;
            let parent = w.parent;
            wire::user32::host_message(&mut state, &mut sched, parent, 0x111, id, hwnd);
            println!("queued BN_CLICKED for Button {id}, hwnd={hwnd:#x}");
        }
        let mut input = false;
        let outcome = loop {
            if !input && args.get(5).is_some_and(|s| s == "input") && sched.threads.len() > 1 {
                if let Some(hwnd) = state.user32.windows.iter().find_map(|(&h, w)| {
                    (w.tid == tid && w.parent == 0 && h != 0x4000fffc).then_some(h)
                }) {
                    wire::user32::host_message(&mut state, &mut sched, hwnd, 0x200, 0, 0x10001);
                    println!("queued host WM_MOUSEMOVE for {hwnd:#x}");
                    input = true;
                }
            }
            let next = if args.get(5).is_some_and(|s| s == "input") && !input {
                (cpu.instr_count + 100000).min(limit)
            } else {
                limit
            };
            let outcome = cpu.run(&mut mem, &mut api, &mut state, &mut log, &mut sched, next);
            if !matches!(outcome, RunOutcome::InstructionLimit) || cpu.instr_count >= limit {
                break outcome;
            }
        };
        println!(
            "resumed: {}; instructions={}; regs={:x?}; eip={:#x}; D3D9={}/{}; displayed={}",
            outcome_text(&outcome),
            cpu.instr_count,
            cpu.regs,
            cpu.eip,
            state.d3d9.draws,
            state.d3d9.frames,
            state.presenter.displayed
        );
        if args.get(8).is_some_and(|s| s == "enter")
            && matches!(outcome, RunOutcome::PresentationLimit(_))
        {
            let hwnd = state.d3d9.last_presented;
            wire::user32::host_message(&mut state, &mut sched, hwnd, 0x100, 13, 1);
            wire::user32::host_message(&mut state, &mut sched, hwnd, 0x102, 13, 1);
            state.stop_after_presents = Some(state.d3d9.frames + 60);
            let outcome = cpu.run(&mut mem, &mut api, &mut state, &mut log, &mut sched, limit);
            println!(
                "Enter held: {}; instructions={}; D3D9={}/{}",
                outcome_text(&outcome),
                cpu.instr_count,
                state.d3d9.draws,
                state.d3d9.frames
            );
            if matches!(outcome, RunOutcome::PresentationLimit(_)) {
                wire::user32::host_message(&mut state, &mut sched, hwnd, 0x101, 13, 0xc0000001);
                state.stop_after_presents = Some(state.d3d9.frames + 120);
                let outcome = cpu.run(&mut mem, &mut api, &mut state, &mut log, &mut sched, limit);
                println!(
                    "Enter released: {}; instructions={}; D3D9={}/{}; displayed={}",
                    outcome_text(&outcome),
                    cpu.instr_count,
                    state.d3d9.draws,
                    state.d3d9.frames,
                    state.presenter.displayed
                );
            }
        }
    }
    println!(
        "PCM: mixed_frames={}; dropped_samples={}; output_stats={:?}; max_pump_gap_ms={}; locked_playback_frames={}; error={:?}",
        state.dsound.mixed_frames, state.dsound.dropped_samples, state.dsound.output_stats, state.dsound.max_pump_gap_ms, state.dsound.locked_playback_frames, state.dsound.error
    );
    println!(
        "Metal: draws={}; error={:?}",
        state.d3d9.gpu_draws(),
        state.d3d9.gpu_error()
    );
    if state.d3d9.frames > 0 {
        let (w, h, pixels) = state
            .graphics
            .window_snapshot(&mem, state.d3d9.last_presented)
            .map_err(|e| format!("snapshot: {e:#x}"))?;
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        for p in pixels {
            ppm.extend([p as u8, (p >> 8) as u8, (p >> 16) as u8]);
        }
        let file = log_dir.join("last-present.ppm");
        std::fs::write(&file, ppm).map_err(|e| e.to_string())?;
        println!("captured {}", file.display());
    }
    log.finish();
    Ok(())
}
