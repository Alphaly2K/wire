//! Condition queues release and reacquire SRW locks without running guest code.
use super::{Block, Threads};
use crate::{
    win32::kernel::srw::{Decision, Operation, SrwLocks},
    x64::memory::{Memory, WRITE},
};
use std::time::{Duration, Instant};

impl Threads {
    /// A blocked call keeps its return frame and completes only after reacquiring.
    pub(in crate::x64::runtime) fn sleep_condition(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        locks: &mut SrwLocks,
        last_error: u64,
    ) -> Result<Option<u64>, String> {
        let condition = args[0];
        let lock = args[1];
        let timeout = args[2] as u32;
        let flags = args[3] as u32;
        let shared = flags == 1;
        if let Some(block) = &self.entries[&self.active].block {
            let Block::Condition {
                condition: waiting,
                lock: released,
                shared: mode,
                timeout: original_timeout,
                notified,
                deadline,
                ..
            } = block
            else {
                return Err("Win64 condition wait conflicts with a blocked operation".into());
            };
            if *waiting != condition
                || *released != lock
                || *mode != shared
                || flags > 1
                || *original_timeout != timeout
            {
                return Err("Win64 condition wait changed while blocked".into());
            }
            let notified = *notified;
            if !notified && !deadline.is_some_and(|d| Instant::now() >= d) {
                return Ok(None);
            }
            // Notification/timeout alone is insufficient: the original lock mode
            // must be restored before either success or ERROR_TIMEOUT is returned.
            mem.check(lock, 8, WRITE)?;
            if !notified {
                mem.check(last_error, 4, WRITE)?;
            }
            let token = mem.read_int(lock, 8)?;
            let Decision::Update(update) =
                locks.prepare(lock, token, self.active, Operation::Acquire { shared })?
            else {
                return Ok(None);
            };
            mem.write_int(lock, 8, update.token())?;
            locks.commit(update);
            if !notified {
                mem.write_int(last_error, 4, 1460)?;
            }
            return Ok(Some(notified as u64));
        }
        if flags > 1 {
            mem.write_int(last_error, 4, 87)?;
            return Ok(Some(0));
        }
        if mem.read_int(condition, 8)? != 0 {
            return Err("unsupported Win64 condition-variable wait list token".into());
        }
        mem.check(lock, 8, WRITE)?;
        let token = mem.read_int(lock, 8)?;
        let Decision::Update(update) =
            locks.prepare(lock, token, self.active, Operation::Release { shared })?
        else {
            unreachable!("SRW release cannot contend");
        };
        if timeout == 0 {
            // No guest thread can run between release and reacquire in this case;
            // ownership validation above is still required, including shared mode.
            mem.write_int(last_error, 4, 1460)?;
            return Ok(Some(0));
        }
        if !self.enabled {
            return Err("Win64 condition wait requires a thread scheduler".into());
        }
        let ticket = self.next_condition_wait;
        let next = ticket
            .checked_add(1)
            .ok_or("Win64 condition wait sequence exhausted")?;
        let deadline =
            (timeout != u32::MAX).then(|| Instant::now() + Duration::from_millis(timeout as u64));
        // All fallible validation precedes release. Cooperative scheduling cannot
        // observe a released lock before the waiter has been registered.
        mem.write_int(lock, 8, update.token())?;
        locks.commit(update);
        self.entries.get_mut(&self.active).unwrap().block = Some(Block::Condition {
            condition,
            lock,
            shared,
            timeout,
            ticket,
            notified: false,
            deadline,
        });
        self.next_condition_wait = next;
        Ok(None)
    }

    pub(in crate::x64::runtime) fn wake_condition(&mut self, condition: u64, all: bool) {
        let now = Instant::now();
        let eligible = |thread: &super::Thread| match &thread.block {
            Some(Block::Condition {
                condition: waiting,
                ticket,
                notified: false,
                deadline,
                ..
            }) if thread.exit.is_none()
                && *waiting == condition
                && deadline.is_none_or(|d| now < d) =>
            {
                Some(*ticket)
            }
            _ => None,
        };
        let first = self
            .entries
            .iter()
            .filter_map(|(&id, thread)| eligible(thread).map(|ticket| (ticket, id)))
            .min()
            .map(|(_, id)| id);
        for (&id, thread) in &mut self.entries {
            if (all || Some(id) == first) && eligible(thread).is_some() {
                if let Some(Block::Condition { notified, .. }) = &mut thread.block {
                    *notified = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        win32::{catalog::InlineSyncApi, x64::inline_sync},
        x64::memory::READ,
    };

    const CV: u64 = 0x510000000;
    const LOCK: u64 = CV + 16;
    const ERROR: u64 = CV + 32;

    fn setup() -> (Threads, Memory, SrwLocks) {
        let mut threads = Threads::new(4096);
        threads.enabled = true;
        for id in [2, 3] {
            threads
                .entries
                .insert(id, Threads::new(4096).entries.remove(&1).unwrap());
        }
        let mut mem = Memory::default();
        mem.map(CV, 4096, READ | WRITE).unwrap();
        (threads, mem, SrwLocks::default())
    }

    fn lock_call(threads: &Threads, mem: &mut Memory, locks: &mut SrwLocks, id: InlineSyncApi) {
        assert!(matches!(
            inline_sync::call(mem, id, &[LOCK], locks, threads.active, ERROR).unwrap(),
            inline_sync::Outcome::Value(_)
        ));
    }

    #[test]
    fn condition_wakes_oldest_waiter_once_and_wake_all_reacquires_shared_locks() {
        let (mut threads, mut mem, mut locks) = setup();
        let args = [CV, LOCK, 1000, 1];
        threads.wake_condition(CV, true); // A wake before registration is not sticky.
        for id in [2, 3, 1] {
            threads.active = id;
            lock_call(&threads, &mut mem, &mut locks, InlineSyncApi::AcquireShared);
            assert_eq!(
                threads
                    .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                    .unwrap(),
                None
            );
            assert_eq!(mem.read_int(LOCK, 8).unwrap(), 0);
        }
        threads.wake_condition(CV + 8, true); // Other addresses cannot wake this queue.
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            None
        );
        threads.wake_condition(CV, false);
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            None
        );
        threads.active = 2;
        if let Some(Block::Condition { deadline, .. }) =
            &mut threads.entries.get_mut(&2).unwrap().block
        {
            *deadline = Some(Instant::now() - Duration::from_secs(1));
        }
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            Some(1)
        );
        threads.entries.get_mut(&2).unwrap().block = None;
        assert_eq!(mem.read_int(LOCK, 8).unwrap(), 6);
        threads.wake_condition(CV, true);
        threads.active = 1;
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            Some(1)
        );
        assert_eq!(mem.read_int(LOCK, 8).unwrap(), 10);
        threads.active = 3;
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            Some(1)
        );
        assert_eq!(mem.read_int(LOCK, 8).unwrap(), 14);
        assert_eq!(mem.read_int(CV, 8).unwrap(), 0);
        for id in [1, 2, 3] {
            threads.active = id;
            lock_call(&threads, &mut mem, &mut locks, InlineSyncApi::ReleaseShared);
        }
        assert_eq!(mem.read_int(LOCK, 8).unwrap(), 0);
    }

    #[test]
    fn expired_condition_wait_cannot_return_until_lock_available_or_consume_a_late_wake() {
        let (mut threads, mut mem, mut locks) = setup();
        let args = [CV, LOCK, 1000, 0];
        lock_call(
            &threads,
            &mut mem,
            &mut locks,
            InlineSyncApi::AcquireExclusive,
        );
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            None
        );
        threads.active = 2;
        lock_call(
            &threads,
            &mut mem,
            &mut locks,
            InlineSyncApi::AcquireExclusive,
        );
        if let Some(Block::Condition { deadline, .. }) =
            &mut threads.entries.get_mut(&1).unwrap().block
        {
            *deadline = Some(Instant::now() - Duration::from_secs(1));
        }
        threads.wake_condition(CV, true);
        threads.active = 1;
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            None
        );
        assert_eq!(mem.read_int(ERROR, 4).unwrap(), 0);
        threads.active = 2;
        lock_call(
            &threads,
            &mut mem,
            &mut locks,
            InlineSyncApi::ReleaseExclusive,
        );
        threads.active = 1;
        assert_eq!(
            threads
                .sleep_condition(&mut mem, &args, &mut locks, ERROR)
                .unwrap(),
            Some(0)
        );
        assert_eq!(mem.read_int(ERROR, 4).unwrap(), 1460);
        assert_eq!(mem.read_int(LOCK, 8).unwrap(), 1);
        lock_call(
            &threads,
            &mut mem,
            &mut locks,
            InlineSyncApi::ReleaseExclusive,
        );
        assert_eq!(mem.read_int(LOCK, 8).unwrap(), 0);
    }
}
