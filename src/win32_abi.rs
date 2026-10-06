//! Wine 10.0 named export specifications. An ABI entry only establishes the
//! calling convention, never that an implementation exists. Stub/ordinal-only
//! entries without parameter types are intentionally not invented.
use std::collections::HashMap;
use std::sync::OnceLock;
/// Resolve only explicit, typed i386 exports. `@` exports have no stable ordinal.
pub(crate) fn ordinal_name(dll: &str, ordinal: u16) -> Option<&'static str> {
    let spec = match dll {
        "oleaut32.dll" => include_str!("../third_party/wine-10.0/abi/oleaut32.spec"),
        "comctl32.dll" => include_str!("../third_party/wine-10.0/abi/comctl32.spec"),
        "shell32.dll" => include_str!("../third_party/wine-10.0/abi/shell32.spec"),
        "shlwapi.dll" => include_str!("../third_party/wine-10.0/abi/shlwapi.spec"),
        "dsound.dll" => include_str!("../third_party/hangover/dsound.spec"),
        _ => return None,
    };
    for line in spec.lines() {
        let Some((prefix, _)) = line.split('#').next()?.split_once('(') else {
            continue;
        };
        let fields: Vec<_> = prefix.split_whitespace().collect();
        if fields.len() >= 3
            && fields[0].parse::<u16>().ok() == Some(ordinal)
            && fields[1] == "stdcall"
        {
            let name = *fields.last()?;
            if stack_bytes(dll, name).is_some() {
                return Some(name);
            }
        }
    }
    None
}
pub(crate) fn stack_bytes(dll: &str, name: &str) -> Option<u32> {
    static TABLE: OnceLock<HashMap<(&'static str, &'static str), u32>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let files = [
            (
                "winmm.dll",
                include_str!("../third_party/hangover/winmm.spec"),
            ),
            (
                "advapi32.dll",
                include_str!("../third_party/hangover/advapi32.spec"),
            ),
            (
                "dsound.dll",
                include_str!("../third_party/hangover/dsound.spec"),
            ),
            (
                "kernel32.dll",
                include_str!("../third_party/wine-10.0/abi/kernel32.spec"),
            ),
            (
                "user32.dll",
                include_str!("../third_party/wine-10.0/abi/user32.spec"),
            ),
            (
                "gdi32.dll",
                include_str!("../third_party/wine-10.0/abi/gdi32.spec"),
            ),
            (
                "shell32.dll",
                include_str!("../third_party/wine-10.0/abi/shell32.spec"),
            ),
            (
                "ole32.dll",
                include_str!("../third_party/wine-10.0/abi/ole32.spec"),
            ),
            (
                "oleaut32.dll",
                include_str!("../third_party/wine-10.0/abi/oleaut32.spec"),
            ),
            (
                "comdlg32.dll",
                include_str!("../third_party/wine-10.0/abi/comdlg32.spec"),
            ),
            (
                "shlwapi.dll",
                include_str!("../third_party/wine-10.0/abi/shlwapi.spec"),
            ),
            (
                "wininet.dll",
                include_str!("../third_party/wine-10.0/abi/wininet.spec"),
            ),
            (
                "comctl32.dll",
                include_str!("../third_party/wine-10.0/abi/comctl32.spec"),
            ),
            (
                "mpr.dll",
                include_str!("../third_party/wine-10.0/abi/mpr.spec"),
            ),
        ];
        let mut entries = HashMap::new();
        for (dll, spec) in files {
            for line in spec.lines() {
                let line = line.split('#').next().unwrap_or("").trim();
                let Some((prefix, rest)) = line.split_once('(') else {
                    continue;
                };
                let fields: Vec<_> = prefix.split_whitespace().collect();
                if fields.len() < 3 || fields[1] != "stdcall" {
                    continue;
                }
                if fields.iter().any(|f| {
                    f.strip_prefix("-arch=")
                        .is_some_and(|a| !a.split(',').any(|a| a == "i386"))
                }) {
                    continue;
                }
                let name = *fields.last().unwrap();
                let Some((args, _)) = rest.split_once(')') else {
                    continue;
                };
                let mut size = 0;
                let mut known = true;
                for kind in args.split_whitespace() {
                    size += match kind {
                        "double" | "int64" => 8,
                        "long" | "ptr" | "str" | "wstr" | "float" | "segptr" | "segstr" => 4,
                        _ => {
                            known = false;
                            0
                        }
                    };
                }
                if known {
                    entries.insert((dll, name), size);
                }
            }
        }
        entries
    });
    table.get(&(dll, name)).copied()
}
