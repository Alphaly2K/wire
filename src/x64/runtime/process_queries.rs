use super::{Memory, Runtime64};
use crate::win32::catalog::ProcessQueryApi;

impl Runtime64 {
    pub(super) fn process_query_call(
        &mut self,
        mem: &mut Memory,
        id: ProcessQueryApi,
        args: &[u64],
    ) -> Result<u64, String> {
        match id {
            ProcessQueryApi::ActiveGroupCount | ProcessQueryApi::MaximumGroupCount => Ok(1),
            ProcessQueryApi::ActiveProcessorCount | ProcessQueryApi::MaximumProcessorCount => {
                if matches!(args[0] as u16, 0 | 0xffff) {
                    Ok(1)
                } else {
                    self.set_error(mem, 87)?;
                    Ok(0)
                }
            }
            _ => unreachable!("only processor counts have Win64 bindings"),
        }
    }
}
