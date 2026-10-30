//! Cooperative critical sections keep ownership in their guest structures.
use super::{call::Call, catalog::CriticalSectionApi as C, context, ServiceError};
use crate::{
    machine::{Mode, State},
    memory::{Memory, WRITE},
};

pub(super) fn dispatch(
    id: C,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    thread: u32,
) -> Result<Option<u64>, ServiceError> {
    let a = &call.arguments;
    let ptr = memory.mode() as usize / 8;
    let length = if ptr == 4 { 24 } else { 40 };
    let owner_at = ptr + 8;
    let spin_at = ptr * 3 + 8;
    let mut section = [0; 40];
    let value = match id {
        C::Initialize | C::InitializeSpin | C::InitializeEx => {
            if id == C::InitializeEx && a[2] & 0xfe000000 != 0 {
                memory.write_int(context::error_address(state, memory.mode()), 4, 87)?;
                return Ok(Some(0));
            }
            section[..ptr].fill(0xff);
            section[ptr..ptr + 4].fill(0xff);
            let spin = if id == C::Initialize { 0 } else { a[1] };
            section[spin_at..spin_at + ptr].copy_from_slice(&spin.to_le_bytes()[..ptr]);
            memory.write(a[0], &section[..length])?;
            u64::from(id != C::Initialize)
        }
        C::SetSpin => {
            memory.read(a[0], &mut section[..length])?;
            let old = u32::from_le_bytes(section[spin_at..spin_at + 4].try_into().unwrap());
            memory.write(a[0] + spin_at as u64, &a[1].to_le_bytes()[..ptr])?;
            u64::from(old)
        }
        C::Enter | C::TryEnter | C::Leave | C::Delete => {
            memory.read(a[0], &mut section[..length])?;
            let count = u32::from_le_bytes(section[ptr + 4..ptr + 8].try_into().unwrap());
            let mut owner = [0; 8];
            owner[..ptr].copy_from_slice(&section[owner_at..owner_at + ptr]);
            let owner = u64::from_le_bytes(owner);
            if (owner == 0) != (count == 0) {
                return Err(ServiceError::Unavailable(
                    "inconsistent critical-section ownership",
                ));
            }
            let count = match id {
                C::Enter | C::TryEnter => {
                    if owner != 0 && owner != u64::from(thread) {
                        if id == C::TryEnter {
                            return Ok(Some(0));
                        }
                        memory.check(a[0] + ptr as u64, ptr + 8, WRITE)?;
                        return Ok(None);
                    }
                    count
                        .checked_add(1)
                        .filter(|&n| n <= i32::MAX as u32)
                        .ok_or(ServiceError::Unavailable(
                            "critical-section recursion overflow",
                        ))?
                }
                C::Leave => {
                    if owner != u64::from(thread) || count == 0 {
                        return Err(ServiceError::Unavailable(
                            "critical section is not owned by the current thread",
                        ));
                    }
                    count - 1
                }
                _ => {
                    if count != 0 {
                        return Err(ServiceError::Unavailable(
                            "cannot delete an owned critical section",
                        ));
                    }
                    0
                }
            };
            section[ptr..ptr + 4].copy_from_slice(&(count as i32 - 1).to_le_bytes());
            section[ptr + 4..ptr + 8].copy_from_slice(&count.to_le_bytes());
            section[owner_at..owner_at + ptr].copy_from_slice(
                &(if count == 0 { 0 } else { u64::from(thread) }).to_le_bytes()[..ptr],
            );
            memory.write(a[0] + ptr as u64, &section[ptr..ptr * 2 + 8])?;
            u64::from(id == C::TryEnter)
        }
    };
    Ok(Some(if memory.mode() == Mode::X86 {
        value as u32 as u64
    } else {
        value
    }))
}
