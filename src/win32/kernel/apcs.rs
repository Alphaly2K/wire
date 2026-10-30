//! Bounded user APC records; memory checks precede queue publication/removal.
use std::collections::VecDeque;
pub(crate) type Queue = VecDeque<(u64, u64)>;
pub(crate) type Call = (u64, [u64; 4]);
pub(crate) const LIMIT: usize = 256;
pub(crate) fn capacity(queue: Result<&mut Queue, u32>) -> Result<&mut Queue, u32> {
    queue.and_then(|queue| {
        if queue.len() >= LIMIT {
            Err(8)
        } else {
            Ok(queue)
        }
    })
}
pub(crate) fn enqueue<E>(
    queue: &mut Queue,
    call: (u64, u64),
    check: impl FnOnce(u64) -> Result<(), E>,
) -> Result<(), E> {
    check(call.0)?;
    queue.push_back(call);
    Ok(())
}
pub(crate) fn take<E>(
    queue: &mut Queue,
    mut check: impl FnMut(u64) -> Result<(), E>,
) -> Result<Vec<Call>, E> {
    for &(target, _) in queue.iter() {
        check(target)?;
    }
    Ok(queue
        .drain(..)
        .map(|(target, data)| (target, [data, 0, 0, 0]))
        .collect())
}
