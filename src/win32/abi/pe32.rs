//! Wine 10.0 named export specifications. An ABI entry only establishes the
//! calling convention, never that an implementation exists. Stub/ordinal-only
//! entries without parameter types are intentionally not invented.
use std::collections::HashMap;
use std::sync::OnceLock;

// Both name/stack and ordinal resolution borrow the same source allocation.
// Repeating include_str! at each consumer can embed whole specs twice when
// their callers land in different codegen units.
static SPECS: [(&str, &str); 14] = [
    (
        "winmm.dll",
        include_str!("../../../third_party/hangover/winmm.spec"),
    ),
    (
        "advapi32.dll",
        include_str!("../../../third_party/hangover/advapi32.spec"),
    ),
    (
        "dsound.dll",
        include_str!("../../../third_party/hangover/dsound.spec"),
    ),
    (
        "kernel32.dll",
        include_str!("../../../third_party/wine-10.0/abi/kernel32.spec"),
    ),
    (
        "user32.dll",
        include_str!("../../../third_party/wine-10.0/abi/user32.spec"),
    ),
    (
        "gdi32.dll",
        include_str!("../../../third_party/wine-10.0/abi/gdi32.spec"),
    ),
    (
        "shell32.dll",
        include_str!("../../../third_party/wine-10.0/abi/shell32.spec"),
    ),
    (
        "ole32.dll",
        include_str!("../../../third_party/wine-10.0/abi/ole32.spec"),
    ),
    (
        "oleaut32.dll",
        include_str!("../../../third_party/wine-10.0/abi/oleaut32.spec"),
    ),
    (
        "comdlg32.dll",
        include_str!("../../../third_party/wine-10.0/abi/comdlg32.spec"),
    ),
    (
        "shlwapi.dll",
        include_str!("../../../third_party/wine-10.0/abi/shlwapi.spec"),
    ),
    (
        "wininet.dll",
        include_str!("../../../third_party/wine-10.0/abi/wininet.spec"),
    ),
    (
        "comctl32.dll",
        include_str!("../../../third_party/wine-10.0/abi/comctl32.spec"),
    ),
    (
        "mpr.dll",
        include_str!("../../../third_party/wine-10.0/abi/mpr.spec"),
    ),
];

/// Resolve only explicit, typed i386 exports. `@` exports have no stable ordinal.
pub(crate) fn ordinal_name(dll: &str, ordinal: u16) -> Option<&'static str> {
    if !matches!(
        dll,
        "oleaut32.dll" | "comctl32.dll" | "shell32.dll" | "shlwapi.dll" | "dsound.dll"
    ) {
        return None;
    }
    let spec = SPECS
        .iter()
        .find_map(|&(provider, spec)| (provider == dll).then_some(spec))?;
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
            if spec_stack_bytes(dll, name).is_some() {
                return Some(name);
            }
        }
    }
    None
}
fn spec_stack_bytes(dll: &str, name: &str) -> Option<u32> {
    static TABLE: OnceLock<HashMap<(&'static str, &'static str), u32>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut entries = HashMap::new();
        for &(dll, spec) in &SPECS {
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

/// Explicit legacy exceptions, then typed i386 specs, then the original CRT policy.
pub(crate) fn stack_bytes(dll: &str, name: &str) -> Option<u32> {
    override_stack_bytes(dll, name)
        .or_else(|| spec_stack_bytes(dll, name))
        .or_else(|| {
            // Existing ABI-known/zero-cleanup policy, including unknown names;
            // execution support is decided separately.
            (dll == "msvcrt.dll"
                || dll == "crtdll.dll"
                || dll == "ucrtbase.dll"
                || dll.starts_with("api-ms-win-crt-")
                || (dll.starts_with("msvcr") && dll.ends_with(".dll")))
            .then_some(0)
        })
}

fn override_stack_bytes(dll: &str, name: &str) -> Option<u32> {
    Some(match (dll, name) {
        // cdecl varargs have no stdcall declaration in the spec parser.
        ("user32.dll", "wsprintfA" | "wsprintfW") => 0,
        // Existing gdi32 aliases lack typed entries in the loaded spec.
        ("gdi32.dll", "TransparentBlt" | "AlphaBlend") => 44,
        ("gdi32.dll", "GradientFill") => 24,
        // Preserve direct ordinal spelling as well as normal name resolution.
        ("oleaut32.dll", "#9") => 4,
        // These providers have legacy ABI entries but no loaded specs.
        ("ws2_32.dll", "closesocket") => 4,
        ("dbghelp.dll", "MiniDumpWriteDump") => 28,
        ("quartz.dll", "AMGetErrorTextW") => 12,
        ("propsys.dll", "PropVariantToDouble") => 8,
        ("mfplat.dll", "MFShutdown") => 0,
        ("mfplat.dll", "MFFrameRateToAverageTimePerFrame") => 12,
        ("mfplat.dll", "MFStartup") => 8,
        (
            "mf.dll",
            "MFCreateVideoRendererActivate" | "MFCreateTopologyNode" | "MFCreateMediaSession",
        ) => 8,
        (
            "mf.dll",
            "MFCreateAudioRendererActivate" | "MFCreateTopology" | "MFCreateSourceResolver",
        ) => 4,
        ("ddraw.dll", "DirectDrawCreate") => 12,
        ("ddraw.dll", "DirectDrawCreateEx") => 16,
        ("ddraw.dll", "DirectDrawEnumerateA") => 8,
        ("dinput.dll", "DirectInputCreateA" | "DirectInputCreateW") => 16,
        ("dinput.dll", "DirectInput8Create") => 20,
        _ => return None,
    })
}
