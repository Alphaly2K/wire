//! Static PE inspection and persisted IR preparation commands.
use std::{path::Path, process::ExitCode};
use wire::pe::PeFile;

pub(super) fn prepare(path: &str, opts: &[String]) -> ExitCode {
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

pub(super) fn imports(path: &str) -> ExitCode {
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
