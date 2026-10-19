//! Command selection; guest process state belongs to Process32/Process64.
mod image;
mod probe64;
mod run32;

use std::process::ExitCode;

pub(crate) fn dispatch(args: &[String]) -> ExitCode {
    if args.len() < 3 {
        eprintln!("wire — Watari-Is-a-Runtime-Environment (M2 spike)");
        eprintln!("usage:");
        eprintln!("  wire imports <program.exe>");
        eprintln!("  wire probe64 <program.exe> [--max-instr N] [--startup] [--dll] [--window] [--cpu-stats] [--api-trace PATH] [--mono-trace PATH] [--guest-arg VALUE] [--graphics-backend PATH --moltenvk PATH] (experimental AMD64 diagnostic)");
        eprintln!("  wire prepare <program.exe> [--out program.wir]");
        eprintln!(
            "  wire run <program.exe> [--ir program.wir] [--op-stats] [--max-instr N] [--log-dir DIR] [--cwd DIR] [--trace-cpu] [--present|--headless] [--software-render] [--capture-frames DIR] [--stop-after-presents N]"
        );
        return ExitCode::from(2);
    }
    match args[1].as_str() {
        "imports" => image::imports(&args[2]),
        "probe64" => probe64::probe(&args[2], &args[3..]),
        "prepare" => image::prepare(&args[2], &args[3..]),
        "run" => run32::run(&args[2], &args[3..]),
        c => {
            eprintln!("unknown command: {c}");
            ExitCode::from(2)
        }
    }
}
