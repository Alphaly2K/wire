//! Validate the PE32 registration chain before any cleanup handler runs.
#[derive(Clone, Copy)]
pub(crate) struct Registration {
    pub address: u32,
    pub next: u32,
    pub handler: u32,
}
pub(crate) enum Error<E> {
    Read(E),
    Invalid(&'static str),
}
pub(crate) fn plan<E>(
    mut head: u32,
    target: u32,
    limits: [u32; 2],
    mut read: impl FnMut(u32) -> Result<u32, E>,
) -> Result<Vec<Registration>, Error<E>> {
    let [limit, top] = limits;
    let mut frames = Vec::new();
    while head != u32::MAX {
        if frames.len() >= 128
            || head & 3 != 0
            || head < limit
            || head.checked_add(8).is_none_or(|end| end > top)
        {
            return Err(Error::Invalid("invalid SEH unwind frame"));
        }
        if head == target {
            break;
        }
        if target != 0 && head > target {
            return Err(Error::Invalid("invalid SEH unwind target"));
        }
        let next = read(head).map_err(Error::Read)?;
        let handler = read(head + 4).map_err(Error::Read)?;
        if next != u32::MAX && next <= head {
            return Err(Error::Invalid("invalid/cyclic SEH unwind chain"));
        }
        frames.push(Registration {
            address: head,
            next,
            handler,
        });
        head = next;
    }
    if target != 0 && head != target {
        return Err(Error::Invalid("SEH unwind target is not in the chain"));
    }
    Ok(frames)
}
