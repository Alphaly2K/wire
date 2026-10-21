//! USER queue preparation, timer posting and wakeups borrow only their owners.
use super::{Message, User32};
use crate::thread::{Sched, ThreadState, WaitTarget, Wake};
use std::{collections::VecDeque, time::Instant};

pub(super) fn clock_ms(start: Instant) -> u32 {
    start.elapsed().as_millis() as u32
}

pub(super) fn message_bits(message: u32) -> u32 {
    match message {
        0x100..=0x109 => 1,
        0x200 => 2,
        0x201..=0x20e => 4,
        0x113 => 0x10,
        0xf => 0x20,
        0xff => 0x400,
        _ => 0x108,
    }
}
impl User32 {
    fn refresh_timers(&mut self, start: Instant, tid: u32) {
        let now = Instant::now();
        let time = clock_ms(start);
        let queue = self.queues.entry(tid).or_default();
        for (&(owner, hwnd, id), timer) in &mut self.timers {
            if owner == tid && timer.deadline <= now {
                timer.deadline = now + timer.period;
                if !queue
                    .iter()
                    .any(|m| m.message == 0x113 && m.hwnd == hwnd && m.wparam == id)
                {
                    queue.push_back(Message {
                        hwnd,
                        message: 0x113,
                        wparam: id,
                        lparam: timer.proc,
                        time,
                        point: self.cursor.position(),
                        synthetic_paint: false,
                        hardware_input: false,
                    });
                    *self.queue_changed.entry(tid).or_default() |= 0x10;
                }
            }
        }
    }

    pub(super) fn put_message(
        &mut self,
        last_error: &mut u32,
        sched: &mut Sched,
        tid: u32,
        message: Message,
    ) -> bool {
        let Some(i) = sched
            .threads
            .iter()
            .position(|t| t.tid == tid && !matches!(t.state, ThreadState::Exited(_)))
        else {
            *last_error = 1444;
            return false;
        };
        let queue = self.queues.entry(tid).or_default();
        if queue.len() >= 10000 {
            *last_error = 1816;
            return false;
        }
        queue.push_back(message);
        let bits = message_bits(message.message);
        *self.queue_changed.entry(tid).or_default() |= bits;
        if matches!(
            sched.threads[i].state,
            ThreadState::Waiting {
                target: WaitTarget::Messages(_),
                ..
            }
        ) || matches!(&sched.threads[i].state, ThreadState::Waiting {
            target: WaitTarget::MessageObjects(_,_,mask),.. } if *mask & bits != 0)
        {
            sched.wake_thread(i, Wake::Retest);
        }
        true
    }

    pub(super) fn prepare_queue(&mut self, start: Instant, tid: u32) -> &mut VecDeque<Message> {
        self.refresh_timers(start, tid);
        let time = clock_ms(start);
        let q = self.queues.entry(tid).or_default();
        q.retain(|m| {
            !m.synthetic_paint
                || self
                    .windows
                    .get(&m.hwnd)
                    .is_some_and(|w| w.update.is_some() && w.style & 0x10000000 != 0)
        });
        for (&hwnd, w) in &self.windows {
            if w.tid == tid
                && w.style & 0x10000000 != 0
                && w.update.is_some()
                && !q.iter().any(|m| m.hwnd == hwnd && m.message == 0xf)
            {
                q.push_back(Message {
                    hwnd,
                    message: 0xf,
                    wparam: 0,
                    lparam: 0,
                    time,
                    point: self.cursor.position(),
                    synthetic_paint: true,
                    hardware_input: false,
                });
                *self.queue_changed.entry(tid).or_default() |= 0x20;
            }
        }
        q
    }

    pub(crate) fn message_wait_ready(
        &mut self,
        start: Instant,
        tid: u32,
        mask: u32,
        old: bool,
    ) -> bool {
        let q = self.prepare_queue(start, tid);
        if old {
            q.iter().any(|m| message_bits(m.message) & mask != 0)
        } else {
            self.queue_changed.get(&tid).copied().unwrap_or(0) & mask != 0
        }
    }
}
