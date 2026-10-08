//! Batch read-only API registry query. TSV in/out avoids starting guest code.
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.expect("coverage input");
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 3 {
            panic!("expected architecture, DLL, name TSV");
        }
        let dll = fields[1].to_ascii_lowercase();
        let known = match fields[0] {
            "x86" => wire::api::native_export(&dll, fields[2]),
            "x64" => {
                let provider = if dll.starts_with("api-ms-win-core-winrt-") {
                    "combase.dll"
                } else if dll.starts_with("api-ms-win-core-") {
                    "kernel32.dll"
                } else {
                    &dll
                };
                wire::x64::runtime::import_supported(provider, fields[2])
            }
            _ => false,
        };
        println!(
            "{line}\t{}",
            if known { "abi_known" } else { "abi_missing" }
        );
    }
}
