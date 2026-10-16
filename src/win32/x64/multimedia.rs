//! Win64 timer capability copy-out and the dedicated period-request owner.
use crate::{
    win32::{catalog::MultimediaApi, kernel::multimedia::PeriodRequests},
    x64::memory::Memory,
};

pub(crate) fn call(
    id: MultimediaApi,
    args: &[u64],
    mem: &mut Memory,
    periods: &mut PeriodRequests,
) -> Result<u64, String> {
    match id {
        MultimediaApi::Caps => {
            if args[1] < 8 || args[0] == 0 {
                Ok(97)
            } else {
                mem.write(
                    args[0],
                    &[1u32.to_le_bytes(), u32::MAX.to_le_bytes()].concat(),
                )?;
                Ok(0)
            }
        }
        MultimediaApi::BeginPeriod => periods.begin(args[0] as u32),
        MultimediaApi::EndPeriod => Ok(periods.end(args[0] as u32)),
    }
}
