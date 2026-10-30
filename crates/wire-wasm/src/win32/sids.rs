//! SID field views and transfers use the same precise guest memory as the JIT.
use super::sid_values as values;
use super::{call::Call, catalog::SidApi as S, context, ServiceError};
use crate::{
    machine::State,
    memory::{Fault, FaultKind, Memory, WRITE},
};

fn at(memory: &Memory, base: u64, offset: u64) -> u64 {
    memory.mode().address(base.wrapping_add(offset))
}
fn valid(memory: &Memory, pointer: u64) -> Result<Option<[u8; 2]>, Fault> {
    if pointer == 0 {
        return Ok(None);
    }
    let mut header = [0; 2];
    for index in 0..2 {
        match memory.read_int(at(memory, pointer, index as u64), 1) {
            Ok(value) => header[index] = value as u8,
            Err(error) if error.kind == FaultKind::Guard => return Err(error),
            Err(_) => return Ok(None),
        }
        // A bad revision must not probe a count byte on a guarded next page.
        if index == 0 && header[0] != 1 {
            return Ok(None);
        }
    }
    Ok(values::valid(&header).then_some(header))
}
pub(super) fn dispatch(
    id: S,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let error = context::error_address(state, memory.mode());
    Ok(match id {
        S::LengthRequired => values::length(a[0] as u8) as u64,
        S::SubAuthority => at(memory, a[0], 8 + u64::from(a[1] as u32) * 4),
        S::SubAuthorityCount => at(memory, a[0], 1),
        S::IdentifierAuthority => at(memory, a[0], 2),
        S::Length => values::length(memory.read_int(at(memory, a[0], 1), 1)? as u8) as u64,
        S::Valid => u64::from(valid(memory, a[0])?.is_some()),
        S::Equal => {
            let Some(first) = valid(memory, a[0])? else {
                return Ok(0);
            };
            let Some(second) = valid(memory, a[1])? else {
                return Ok(0);
            };
            if first[1] != second[1] {
                return Ok(0);
            }
            let mut left = vec![0; values::length(first[1])];
            let mut right = vec![0; left.len()];
            memory.read(a[0], &mut left)?;
            memory.read(a[1], &mut right)?;
            u64::from(left == right)
        }
        S::Copy => {
            let length = values::length(memory.read_int(at(memory, a[2], 1), 1)? as u8);
            if (a[0] as u32 as usize) < length {
                memory.write_int(error, 4, 122)?;
                return Ok(0);
            }
            let mut bytes = vec![0; length];
            memory.read(a[2], &mut bytes)?;
            memory.write(a[1], &bytes)?;
            1
        }
        S::Initialize => {
            let count = a[2] as u8;
            if count > 15 {
                memory.write_int(error, 4, 87)?;
                return Ok(0);
            }
            let mut outputs = Vec::new();
            if a[1] != 0 {
                let mut authority = [0; 6];
                memory.read(a[1], &mut authority)?;
                outputs.push((a[0], values::header(count, authority).to_vec()));
            } else {
                outputs.push((a[0], vec![1, count]));
            }
            if count != 0 {
                outputs.push((at(memory, a[0], 8), vec![0; count as usize * 4]));
            }
            for (p, bytes) in &outputs {
                memory.check(*p, bytes.len(), WRITE)?;
            }
            for (p, bytes) in outputs {
                memory
                    .write(p, &bytes)
                    .expect("preflighted SID initialization");
            }
            1
        }
    })
}
