//! PE32 field ordering is intentionally distinct from Win64's checked codec.
use crate::{mem::GuestMem, win32::catalog::CriticalSectionApi};

pub(crate) enum Outcome {
    Value(u32),
    Block { address: u32, owner: u32 },
    WakeOne(u32),
}

pub(crate) fn call(
    id: CriticalSectionApi,
    args: &[u32],
    mem: &mut GuestMem,
    thread: u32,
) -> Result<Outcome, String> {
    use CriticalSectionApi::*;
    let cs = args[0];
    let value = match id {
        Initialize | InitializeSpin | InitializeEx => {
            // Preserve sequential writes and their observable partial failures.
            let spin = if id == Initialize { 0 } else { args[1] };
            for (offset, value) in [(0, 0), (4, u32::MAX), (8, 0), (12, 0), (16, 0), (20, spin)] {
                mem.write_u32(cs + offset, value)
                    .map_err(|e| e.to_string())?;
            }
            (id != Initialize) as u32
        }
        Enter | TryEnter => {
            let owner = mem.read_u32(cs + 12).map_err(|e| e.to_string())?;
            if owner == 0 || owner == thread {
                let rc = mem.read_u32(cs + 8).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 12, thread).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 8, rc + 1).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 4, 0).map_err(|e| e.to_string())?;
                (id == TryEnter) as u32
            } else if id == TryEnter {
                0
            } else {
                return Ok(Outcome::Block { address: cs, owner });
            }
        }
        Leave => {
            let rc = mem
                .read_u32(cs + 8)
                .map_err(|e| e.to_string())?
                .saturating_sub(1);
            mem.write_u32(cs + 8, rc).map_err(|e| e.to_string())?;
            if rc == 0 {
                mem.write_u32(cs + 12, 0).map_err(|e| e.to_string())?;
                mem.write_u32(cs + 4, u32::MAX).map_err(|e| e.to_string())?;
                return Ok(Outcome::WakeOne(cs));
            }
            0
        }
        Delete => {
            mem.write_u32(cs + 4, u32::MAX).map_err(|e| e.to_string())?;
            mem.write_u32(cs + 12, 0).map_err(|e| e.to_string())?;
            0
        }
        SetSpin => unreachable!("PE32 spin update remains ABI-only"),
    };
    Ok(Outcome::Value(value))
}
