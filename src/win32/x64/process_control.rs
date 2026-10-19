//! Restricted child creation probe; CPU completion and diagnostics remain explicit controls.
use super::text::utf16;
use crate::{
    fs::GuestFs,
    x64::memory::{Memory, READ, WRITE},
};

pub(crate) enum ChildProbe {
    InvalidStartup,
    Missing { application: String, error: u32 },
}
pub(crate) fn probe_child(
    mem: &Memory,
    args: &[u64],
    filesystem: Option<&GuestFs>,
) -> Result<ChildProbe, String> {
    // An existing executable still requires a real child-process backend.
    mem.check(args[8], 104, READ)?;
    mem.check(args[9], 24, WRITE)?;
    let cb = mem.read_int(args[8], 4)?;
    if cb != 104 && cb != 112 {
        return Ok(ChildProbe::InvalidStartup);
    }
    let application = if args[0] != 0 {
        utf16(mem, args[0])?
    } else {
        let command = utf16(mem, args[1])?;
        let command = command.trim_start();
        if let Some(quoted) = command.strip_prefix('"') {
            quoted
                .split_once('"')
                .ok_or("unterminated Win64 child executable quote")?
                .0
                .to_owned()
        } else {
            let first = command
                .split_whitespace()
                .next()
                .ok_or("empty Win64 child command line")?;
            if !first.to_ascii_lowercase().ends_with(".exe") {
                return Err(
                    "Win64 ambiguous unquoted child executable search not implemented".into(),
                );
            }
            first.to_owned()
        }
    };
    let fs = filesystem.ok_or("Win64 CreateProcess has no mounted guest filesystem")?;
    let result = fs
        .resolve(&application)
        .and_then(|path| std::fs::metadata(path).map_err(|e| crate::fs::win_error(&e)));
    match result {
        Err(error) => Ok(ChildProbe::Missing { application, error }),
        Ok(_) => Err(format!(
            "Win64 child-process execution not implemented: {application}"
        )),
    }
}
