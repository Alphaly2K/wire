//! Export the complete legacy contract and current Wasm binding matrix.
use std::collections::BTreeSet;
use std::io::{self, BufRead};
use wire_wasm::{
    machine::Mode,
    pe::linker,
    win32::{
        catalog::{self, Architecture},
        provider_aliases, Services,
    },
};

fn main() {
    if std::env::args().any(|arg| arg == "--probe") {
        println!("mode\tdll\tname\tbound\tbuiltin_provider");
        for line in io::stdin().lock().lines() {
            let line = line.expect("binding probe input");
            let (dll, name) = line.split_once('\t').expect("DLL and export name");
            let canonical = linker::module_name(dll);
            for mode in [Mode::X86, Mode::X64] {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    mode as u32,
                    dll,
                    name,
                    canonical
                        .as_ref()
                        .is_ok_and(|dll| Services::new(mode).bind(dll, name).is_ok()),
                    canonical
                        .as_ref()
                        .is_ok_and(|dll| linker::provider(dll, mode))
                );
            }
        }
        return;
    }
    let providers: BTreeSet<_> = catalog::contracts()
        .flat_map(|c| {
            c.dlls(Architecture::X86)
                .iter()
                .chain(c.dlls(Architecture::X64))
        })
        .copied()
        .chain(provider_aliases::providers())
        .collect();
    println!(
        "mode\tfamily\tid\tname\tlegacy_execution\tlegacy_static\tlegacy_dynamic\twasm_bindings"
    );
    for (mode, architecture) in [
        (Mode::X86, Architecture::X86),
        (Mode::X64, Architecture::X64),
    ] {
        for c in catalog::contracts() {
            let id = format!("{:?}", c.id);
            let family = id.split('(').next().unwrap();
            let bound: Vec<_> = providers
                .iter()
                .copied()
                .filter(|dll| Services::new(mode).bind(dll, c.name).is_ok())
                .collect();
            println!(
                "{}\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}",
                mode as u32,
                family,
                id,
                c.name,
                c.execution(architecture),
                c.dlls(architecture).join(","),
                c.lookup_dlls(architecture).join(","),
                bound.join(",")
            );
        }
    }
}
