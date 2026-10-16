//! Checked inline-state codecs; CPU frames, retries and tracing stay in Process.
use crate::{
    win32::{
        catalog::InlineSyncApi,
        kernel::srw::{Decision, Operation, SrwLocks},
    },
    x64::memory::{Memory, WRITE},
};

pub(crate) enum Outcome {
    Value(u64),
    TryFailed,
    BlockSrw { pointer: u64, shared: bool },
    BlockOnce(u64),
}

pub(crate) fn call(
    mem: &mut Memory,
    id: InlineSyncApi,
    args: &[u64],
    locks: &mut SrwLocks,
    thread: u64,
    last_error: u64,
) -> Result<Outcome, String> {
    use InlineSyncApi::*;
    let value = match id {
        InitializeCondition => {
            mem.write_int(args[0], 8, 0)?;
            0
        }
        WakeCondition | WakeAllCondition => {
            if mem.read_int(args[0], 8)? != 0 {
                return Err(
                    "Win64 condition-variable wait list requires a thread scheduler".into(),
                );
            }
            0
        }
        InitializeSrw | AcquireExclusive | AcquireShared | ReleaseExclusive | ReleaseShared
        | TryExclusive | TryShared => {
            let pointer = args[0];
            mem.check(pointer, 8, WRITE)?;
            let token = mem.read_int(pointer, 8)?;
            let shared = matches!(id, AcquireShared | ReleaseShared | TryShared);
            let attempt = matches!(id, TryExclusive | TryShared);
            let operation = match id {
                InitializeSrw => Operation::Initialize,
                ReleaseExclusive | ReleaseShared => Operation::Release { shared },
                _ => Operation::Acquire { shared },
            };
            match locks.prepare(pointer, token, thread, operation)? {
                Decision::Contended => {
                    return Ok(if attempt {
                        Outcome::TryFailed
                    } else {
                        Outcome::BlockSrw { pointer, shared }
                    });
                }
                Decision::Update(update) => {
                    mem.write_int(pointer, 8, update.token())?;
                    locks.commit(update);
                }
            }
            attempt as u64
        }
        BeginOnce => {
            if args[1] & !1 != 0 {
                return Err("Win64 asynchronous InitOnce not implemented".into());
            }
            let state = mem.read_int(args[0], 8)?;
            mem.check(args[2], 4, WRITE)?;
            if args[3] != 0 {
                mem.check(args[3], 8, WRITE)?;
            }
            if state & 3 == 2 {
                mem.write_int(args[2], 4, 0)?;
                if args[3] != 0 {
                    mem.write_int(args[3], 8, state & !3)?;
                }
                1
            } else if args[1] & 1 != 0 {
                mem.write_int(last_error, 4, 31)?;
                0
            } else if state == 0 {
                mem.check(args[0], 8, WRITE)?;
                mem.write_int(args[0], 8, 1)?;
                mem.write_int(args[2], 4, 1)?;
                if args[3] != 0 {
                    mem.write_int(args[3], 8, 0)?;
                }
                1
            } else {
                return Ok(Outcome::BlockOnce(args[0]));
            }
        }
        CompleteOnce => {
            let failed = args[1] & 4 != 0;
            if args[1] & !4 != 0
                || args[2] & 3 != 0
                || (failed && args[2] != 0)
                || mem.read_int(args[0], 8)? != 1
            {
                mem.write_int(last_error, 4, 87)?;
                0
            } else {
                mem.write_int(args[0], 8, if failed { 0 } else { args[2] | 2 })?;
                1
            }
        }
    };
    Ok(Outcome::Value(value))
}
