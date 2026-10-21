//! Scalar thread operations borrow only their owner, memory and current error slot.
use super::Threads;
use crate::{
    win32::catalog::ThreadPropertyApi,
    x64::memory::{Memory, WRITE},
};

impl Threads {
    pub fn property_call(
        &mut self,
        mem: &mut Memory,
        id: ThreadPropertyApi,
        args: &[u64],
        error_slot: u64,
    ) -> Result<u64, String> {
        use ThreadPropertyApi::*;
        if id == Open {
            let thread = args[2] as u32 as u64;
            let retained = self.entries.get(&thread).is_some_and(|entry| {
                entry.exit.is_none() || self.handles.values().any(|&id| id == thread)
            });
            let result = if !retained {
                Err(87)
            } else if args[0] as u32 & !0xf21f_ffff != 0 {
                Err(50)
            } else {
                self.alias(thread, args[1] as u32 != 0)
            };
            return match result {
                Ok(handle) => Ok(handle),
                Err(error) => {
                    mem.write_int(error_slot, 4, error as u64)?;
                    Ok(0)
                }
            };
        }
        if id == StackGuarantee {
            let requested = mem.read_int(args[0], 4)?;
            if requested == 0 {
                // No exception-handler stack reserve exists in this profile.
                mem.write_int(args[0], 4, 0)?;
                return Ok(1);
            }
            mem.write_int(error_slot, 4, 50)?;
            return Ok(0);
        }
        if id == GetErrorMode {
            return Ok(self.entries[&self.active].error_mode as u64);
        }
        if id == SetErrorMode {
            if args[0] & !0x8007 != 0 {
                mem.write_int(error_slot, 4, 87)?;
                return Ok(0);
            }
            if args[1] != 0 {
                mem.check(args[1], 4, WRITE)?;
            }
            let old = self.entries[&self.active].error_mode;
            if args[1] != 0 {
                mem.write_int(args[1], 4, old as u64)?;
            }
            self.entries.get_mut(&self.active).unwrap().error_mode = args[0] as u32;
            return Ok(1);
        }
        let thread = match self.id(args[0]) {
            Err(error) => {
                mem.write_int(error_slot, 4, error as u64)?;
                return Ok(match id {
                    Resume | Suspend => u32::MAX as u64,
                    GetPriority => 0x7fffffff,
                    _ => 0,
                });
            }
            Ok(thread) => thread,
        };
        let t = self.entries.get_mut(&thread).unwrap();
        Ok(match id {
            Id => thread,
            GetPriority => t.priority as u32 as u64,
            SetPriority => {
                let priority = args[1] as i32;
                if !matches!(priority, -15 | -2..=2 | 15) {
                    mem.write_int(error_slot, 4, 87)?;
                    0
                } else {
                    t.priority = priority;
                    1
                }
            }
            ExitCode => {
                mem.write_int(args[1], 4, t.exit.unwrap_or(259) as u64)?;
                1
            }
            SetAffinity => {
                if args[1] != 1 {
                    mem.write_int(error_slot, 4, 87)?;
                    0
                } else {
                    1
                }
            }
            Suspend => {
                let old = t.suspended;
                if old >= 127 {
                    mem.write_int(error_slot, 4, 87)?;
                    u32::MAX as u64
                } else {
                    t.suspended += 1;
                    old as u64
                }
            }
            Resume => {
                let old = t.suspended;
                t.suspended = old.saturating_sub(1);
                old as u64
            }
            Open | GetErrorMode | SetErrorMode | StackGuarantee => {
                unreachable!("current-thread properties handled first")
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{PAGE, READ};

    #[test]
    fn open_thread_retains_exited_identity_and_shares_alias_capacity_without_mutating_on_error() {
        let mut threads = Threads::new(65536);
        let mut mem = Memory::default();
        let error = 0x600000000;
        mem.map(error, PAGE, READ | WRITE).unwrap();
        let open = |t: &mut Threads, m: &mut Memory| {
            t.property_call(m, ThreadPropertyApi::Open, &[0x1fffff, 0, 1], error)
                .unwrap()
        };
        let first = open(&mut threads, &mut mem);
        threads.entries.get_mut(&1).unwrap().exit = Some(42);
        let second = open(&mut threads, &mut mem);
        assert_ne!(first, second);
        assert_ne!(second, 0);
        threads.close(first).unwrap();
        assert_eq!(threads.id(second), Ok(1));
        assert_eq!(threads.entries[&1].exit, Some(42));
        threads.close(second).unwrap();
        let cursor = threads.next_alias;
        assert_eq!(open(&mut threads, &mut mem), 0);
        assert_eq!(mem.read_int(error, 4).unwrap(), 87);
        assert_eq!(threads.next_alias, cursor);
        assert!(threads.handles.is_empty());
        threads.entries.get_mut(&1).unwrap().exit = None;
        for _ in 0..1024 {
            assert_ne!(open(&mut threads, &mut mem), 0);
        }
        let cursor = threads.next_alias;
        assert_eq!(open(&mut threads, &mut mem), 0);
        assert_eq!(mem.read_int(error, 4).unwrap(), 8);
        assert_eq!(threads.next_alias, cursor);
        mem.protect(error, PAGE, READ).unwrap();
        assert!(threads
            .property_call(&mut mem, ThreadPropertyApi::Open, &[0, 0, 1], error)
            .is_err());
        assert_eq!(threads.handles.len(), 1024);
        assert_eq!(threads.next_alias, cursor);
    }
}
