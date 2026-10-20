use super::{threads::Threads, Memory};
use crate::win32::catalog::ProcessQueryApi;

pub(super) fn call(
    mem: &mut Memory,
    id: ProcessQueryApi,
    args: &[u64],
    threads: &Threads,
) -> Result<u64, String> {
    match id {
        ProcessQueryApi::ActiveGroupCount | ProcessQueryApi::MaximumGroupCount => Ok(1),
        ProcessQueryApi::ActiveProcessorCount | ProcessQueryApi::MaximumProcessorCount => {
            if matches!(args[0] as u16, 0 | 0xffff) {
                Ok(1)
            } else {
                threads.set_error(mem, 87)?;
                Ok(0)
            }
        }
        _ => unreachable!("only processor counts have Win64 bindings"),
    }
}
