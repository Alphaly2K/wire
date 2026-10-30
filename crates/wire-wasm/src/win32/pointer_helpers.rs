use super::{call::Call, catalog::PointerHelperApi as P, pointer_values};
use crate::memory::{Memory, EXECUTE, READ, WRITE};

pub(super) fn dispatch(id: P, call: &Call, memory: &Memory) -> u64 {
    let a = &call.arguments;
    match id {
        P::Encode | P::Decode => a[0],
        P::MulDiv => pointer_values::mul_div(a[0] as u32, a[1] as u32, a[2] as u32).into(),
        P::BadRead | P::BadWrite => {
            if a[1] == 0 {
                return 0;
            }
            let Ok(size) = usize::try_from(a[1]) else {
                return 1;
            };
            u64::from(
                memory
                    .check(a[0], size, if id == P::BadWrite { WRITE } else { READ })
                    .is_err(),
            )
        }
        P::BadCode => u64::from(!memory.probe(a[0], 1, EXECUTE)),
        P::FlushCache => unreachable!("cache flush uses the VMM adapter"),
    }
}
