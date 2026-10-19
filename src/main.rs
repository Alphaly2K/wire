mod cli;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    cli::dispatch(&args)
}
