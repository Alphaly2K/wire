//! Win64 structure codec and ownership checks; Process commits control outcomes.
use crate::{
    win32::catalog::CriticalSectionApi,
    x64::memory::{Memory, READ, WRITE},
};

pub(crate) enum Outcome {
    Value(u64),
    TryFailed,
    Block(u64),
}

pub(crate) fn call(
    mem: &mut Memory,
    id: CriticalSectionApi,
    args: &[u64],
    thread: u64,
) -> Result<Outcome, String> {
    use CriticalSectionApi::*;
    let value = match id {
        Initialize | InitializeSpin | InitializeEx => {
            // Low 24 flags are ignored by the existing Windows 10 subset.
            if id == InitializeEx && args[2] & 0xfe000000 != 0 {
                return Err(format!(
                    "unsupported Win64 critical section flags {:#x}",
                    args[2]
                ));
            }
            let mut section = [0; 40];
            section[..8].copy_from_slice(&u64::MAX.to_le_bytes());
            section[8..12].copy_from_slice(&(-1i32).to_le_bytes());
            section[32..40]
                .copy_from_slice(&(args.get(1).copied().unwrap_or(0) as u32 as u64).to_le_bytes());
            mem.write(args[0], &section)?;
            (id != Initialize) as u64
        }
        SetSpin => {
            mem.check(args[0], 40, READ)?;
            mem.check(args[0] + 32, 8, WRITE)?;
            let old = mem.read_int(args[0] + 32, 8)?;
            mem.write_int(args[0] + 32, 8, args[1])?;
            old
        }
        Enter | TryEnter | Leave | Delete => {
            let mut section = [0; 40];
            mem.read(args[0], &mut section)?;
            let owner = u64::from_le_bytes(section[16..24].try_into().unwrap());
            let count = u32::from_le_bytes(section[12..16].try_into().unwrap());
            let count = if matches!(id, Enter | TryEnter) {
                if owner != 0 && owner != thread {
                    if id == TryEnter {
                        return Ok(Outcome::TryFailed);
                    }
                    mem.check(args[0], 40, WRITE)?;
                    return Ok(Outcome::Block(args[0]));
                }
                count
                    .checked_add(1)
                    .filter(|&n| n <= i32::MAX as u32)
                    .ok_or("critical section recursion overflow")?
            } else if id == Leave {
                if owner != thread || count == 0 {
                    return Err("Win64 critical section not owned".into());
                }
                count - 1
            } else {
                if count != 0 {
                    return Err("deleting an owned Win64 critical section".into());
                }
                0
            };
            section[8..12].copy_from_slice(&(count as i32 - 1).to_le_bytes());
            section[12..16].copy_from_slice(&count.to_le_bytes());
            section[16..24].copy_from_slice(&(if count == 0 { 0 } else { thread }).to_le_bytes());
            mem.write(args[0], &section)?;
            (id == TryEnter) as u64
        }
    };
    Ok(Outcome::Value(value))
}
