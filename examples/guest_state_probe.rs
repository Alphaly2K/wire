//! Observe selected guest bytes between execution slices without changing them.
use std::{io::Write, path::Path, time::Instant};
use wire::x64::{
    memory::Memory,
    process::{Outcome, Process64},
};

struct Watch {
    label: String,
    chain: Vec<u64>,
    size: usize,
    previous: Option<String>,
}

fn number(text: &str) -> Result<u64, String> {
    text.strip_prefix("0x").map_or_else(
        || text.parse::<u64>().map_err(|e| e.to_string()),
        |hex| u64::from_str_radix(hex, 16).map_err(|e| e.to_string()),
    )
}

impl Watch {
    fn parse(text: &str) -> Result<Self, String> {
        let (label, spec) = text
            .split_once('@')
            .ok_or("watch needs LABEL@ADDRESS[/OFFSET...]:SIZE")?;
        let (chain, size) = spec.rsplit_once(':').ok_or("watch needs a byte count")?;
        let chain = chain
            .split('/')
            .map(number)
            .collect::<Result<Vec<_>, _>>()?;
        let size = usize::try_from(number(size)?).map_err(|e| e.to_string())?;
        if chain.is_empty() || chain.len() > 8 || !(1..=256).contains(&size) {
            return Err("watch allows 1..8 addresses and 1..256 bytes".into());
        }
        Ok(Self {
            label: label.into(),
            chain,
            size,
            previous: None,
        })
    }

    fn read(&self, memory: &Memory) -> Result<String, String> {
        let mut address = self.chain[0];
        for offset in &self.chain[1..] {
            let mut bytes = [0; 8];
            memory.observe(address, &mut bytes)?;
            address = u64::from_le_bytes(bytes)
                .checked_add(*offset)
                .ok_or("watch address overflow")?;
        }
        let mut bytes = vec![0; self.size];
        memory.observe(address, &mut bytes)?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Ok(format!("{address:#x}:{hex}"))
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let exe = args
        .next()
        .ok_or("needs EXE BACKEND MOLTENVK [--watch SPEC] [--slice N] [--seconds N] [--desktop-size WIDTHxHEIGHT]")?;
    let backend = args.next().ok_or("needs graphics backend")?;
    let moltenvk = args.next().ok_or("needs MoltenVK")?;
    let mut watches = Vec::new();
    let mut slice = 1000;
    let mut seconds = 1800;
    let mut desktop = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("missing option value")?;
        match flag.as_str() {
            "--watch" => watches.push(Watch::parse(&value)?),
            "--slice" => slice = number(&value)?,
            "--seconds" => seconds = number(&value)?,
            "--desktop-size" => {
                let (width, height) = value.split_once('x').ok_or("needs WIDTHxHEIGHT")?;
                desktop = Some([
                    width.parse::<u32>().map_err(|e| e.to_string())?,
                    height.parse::<u32>().map_err(|e| e.to_string())?,
                ]);
            }
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    if slice == 0 || watches.len() > 64 {
        return Err("slice must be positive; at most 64 watches".into());
    }
    let mut process = Process64::load(Path::new(&exe), false)?;
    if let Some([width, height]) = desktop {
        process.runtime.configure_desktop(width, height)?;
    }
    process
        .runtime
        .attach_graphics(wire::gpu::NativeGraphics::load(
            Path::new(&backend),
            Path::new(&moltenvk),
        )?)?;
    process.enable_window()?;
    let start = Instant::now();
    let mut next_progress = 0;
    while start.elapsed().as_secs() < seconds {
        let outcome = process
            .run(process.cpu.work.saturating_add(slice))
            .map_err(|e| {
                format!(
                    "{e}; RIP={:#x} work={} regs={:x?}",
                    process.cpu.rip, process.cpu.work, process.cpu.regs
                )
            })?;
        for watch in &mut watches {
            let value = watch
                .read(&process.memory)
                .unwrap_or_else(|e| format!("unavailable:{e}"));
            if watch.previous.as_ref() != Some(&value) {
                println!(
                    "watch\t{}\t{}\t{:#x}\t{}\t{}",
                    start.elapsed().as_millis(),
                    process.cpu.work,
                    process.cpu.rip,
                    watch.label,
                    value
                );
                watch.previous = Some(value);
            }
        }
        if process.cpu.work >= next_progress {
            println!(
                "progress\t{}\t{}\t{:#x}\t{}",
                start.elapsed().as_millis(),
                process.cpu.work,
                process.cpu.rip,
                process.phase()
            );
            next_progress = process.cpu.work.saturating_add(25_000_000);
            std::io::stdout().flush().map_err(|e| e.to_string())?;
        }
        if outcome != Outcome::Budget {
            println!("outcome: {outcome:?}");
            return Ok(());
        }
    }
    Err("observation deadline reached".into())
}
