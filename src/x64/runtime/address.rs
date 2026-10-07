//! Cooperative WaitOnAddress queues. Guest code runs only in Process64.
use super::{threads::Block, Memory, Runtime64};
use std::time::{Duration, Instant};
impl Runtime64 {
    /// None means the validated gateway remains blocked without consuming its frame.
    pub(super) fn wait_address(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
    ) -> Result<Option<u64>, String> {
        if !matches!(args[2], 1 | 2 | 4 | 8) {
            self.set_error(mem, 87)?;
            return Ok(Some(0));
        }
        if let Some(Block::Address {
            address,
            notified,
            deadline,
            ..
        }) = &self.threads.entries[&self.threads.active].block
        {
            if *address != args[0] {
                return Err("Win64 address wait changed while blocked".into());
            }
            if *notified {
                return Ok(Some(1));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                self.set_error(mem, 1460)?;
                return Ok(Some(0));
            }
            return Ok(None);
        }
        // Compare all SIZE_T-selected bytes before enqueuing. No guest instruction
        // or other thread can run between comparison and queue registration.
        let current = mem.read_int(args[0], args[2] as usize)?;
        let compared = mem.read_int(args[1], args[2] as usize)?;
        if current != compared {
            return Ok(Some(1));
        }
        if args[3] == 0 {
            self.set_error(mem, 1460)?;
            return Ok(Some(0));
        }
        let ticket = self.threads.next_address_wait;
        let next = ticket
            .checked_add(1)
            .ok_or("Win64 address wait sequence exhausted")?;
        self.block(Block::Address {
            address: args[0],
            ticket,
            notified: false,
            deadline: (args[3] != u32::MAX as u64)
                .then(|| Instant::now() + Duration::from_millis(args[3])),
        })?;
        self.threads.next_address_wait = next;
        Ok(None)
    }
    pub(super) fn wake_address(&mut self, address: u64, all: bool) {
        let now = Instant::now();
        let first = self
            .threads
            .entries
            .iter()
            .filter_map(|(&id, thread)| match &thread.block {
                Some(Block::Address {
                    address: waiting,
                    ticket,
                    notified: false,
                    deadline,
                }) if thread.exit.is_none()
                    && *waiting == address
                    && deadline.is_none_or(|d| now < d) =>
                {
                    Some((*ticket, id))
                }
                _ => None,
            })
            .min()
            .map(|(_, id)| id);
        for (&id, thread) in &mut self.threads.entries {
            if all || Some(id) == first {
                if let Some(Block::Address {
                    address: waiting,
                    notified,
                    deadline,
                    ..
                }) = &mut thread.block
                {
                    if *waiting == address
                        && thread.exit.is_none()
                        && deadline.is_none_or(|d| now < d)
                    {
                        *notified = true;
                    }
                }
            }
        }
    }
}
