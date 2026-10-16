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
            GetErrorMode | SetErrorMode => unreachable!("current-thread mode handled first"),
        })
    }
}
