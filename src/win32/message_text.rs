//! Portable English system messages retained by the Windows compatibility layer.
pub(crate) fn system(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "The operation completed successfully.",
        2 => "The system cannot find the file specified.",
        3 => "The system cannot find the path specified.",
        5 => "Access is denied.",
        6 => "The handle is invalid.",
        8 => "Not enough memory resources are available to process this command.",
        32 => "The process cannot access the file because it is being used by another process.",
        50 => "The request is not supported.",
        87 => "The parameter is incorrect.",
        122 => "The data area passed to a system call is too small.",
        126 => "The specified module could not be found.",
        127 => "The specified procedure could not be found.",
        183 => "Cannot create a file when that file already exists.",
        203 => "The system could not find the environment option that was entered.",
        _ => return None,
    })
}
