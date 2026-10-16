//! Process-owned SRW bookkeeping. Guest memory and scheduling belong to adapters.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Default)]
struct State {
    owner: u64,
    readers: u32,
}
impl State {
    fn token(self) -> u64 {
        if self.owner != 0 {
            1
        } else if self.readers != 0 {
            self.readers as u64 * 4 + 2
        } else {
            0
        }
    }
}

pub(crate) enum Operation {
    Initialize,
    Acquire { shared: bool },
    Release { shared: bool },
}

pub(crate) struct Update {
    pointer: u64,
    state: State,
}
impl Update {
    pub(crate) fn token(&self) -> u64 {
        self.state.token()
    }
}

pub(crate) enum Decision {
    Update(Update),
    Contended,
}

#[derive(Default)]
pub(crate) struct SrwLocks {
    entries: BTreeMap<u64, State>,
}
impl SrwLocks {
    pub(crate) fn prepare(
        &self,
        pointer: u64,
        token: u64,
        thread: u64,
        operation: Operation,
    ) -> Result<Decision, String> {
        let State { owner, readers } = self.entries.get(&pointer).copied().unwrap_or_default();
        if !matches!(operation, Operation::Initialize)
            && token != (State { owner, readers }).token()
        {
            return Err("invalid Win64 SRW lock state".into());
        }
        let state = match operation {
            Operation::Initialize => State::default(),
            Operation::Release { shared: true } => {
                if readers == 0 || owner != 0 {
                    return Err("unowned Win64 shared SRW lock".into());
                }
                State {
                    owner: 0,
                    readers: readers - 1,
                }
            }
            Operation::Release { shared: false } => {
                if owner != thread || readers != 0 {
                    return Err("unowned Win64 exclusive SRW lock".into());
                }
                State::default()
            }
            Operation::Acquire { shared } => {
                if owner != 0 || (!shared && readers != 0) {
                    return Ok(Decision::Contended);
                }
                if shared {
                    State {
                        owner: 0,
                        readers: readers.checked_add(1).ok_or("SRW reader overflow")?,
                    }
                } else {
                    State {
                        owner: thread,
                        readers: 0,
                    }
                }
            }
        };
        Ok(Decision::Update(Update { pointer, state }))
    }

    /// Called only after the adapter successfully publishes the guest token.
    pub(crate) fn commit(&mut self, update: Update) {
        if update.token() == 0 {
            self.entries.remove(&update.pointer);
        } else {
            self.entries.insert(update.pointer, update.state);
        }
    }

    pub(crate) fn available(&self, pointer: u64, shared: bool) -> bool {
        let State { owner, readers } = self.entries.get(&pointer).copied().unwrap_or_default();
        owner == 0 && (shared || readers == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader_overflow_preserves_maximum_token_and_exclusive_contention() {
        let state = State {
            owner: 0,
            readers: u32::MAX,
        };
        let mut locks = SrwLocks::default();
        locks.entries.insert(8, state);
        assert!(
            matches!(locks.prepare(8, state.token(), 1, Operation::Acquire { shared: true }),
            Err(error) if error == "SRW reader overflow")
        );
        assert_eq!(locks.entries[&8].token(), state.token());
        assert!(locks.available(8, true));
        assert!(!locks.available(8, false));
    }
}
