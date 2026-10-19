//! PE32 object readiness, acquisition and scheduler wake transitions.
use crate::{
    thread::{target_mentions, Sched, ThreadState, Wake},
    win32::kernel::objects32::{HandleObj, KernelObjects32},
};

/// Non-consuming readiness test (used by wait-all and by acquire itself).
pub(crate) fn obj_ready(handles: &KernelObjects32, sched: &Sched, h: u32) -> bool {
    match handles.get(&h) {
        Some(HandleObj::Event { signaled, .. }) => *signaled,
        Some(HandleObj::Mutex { owner, .. }) => {
            owner.is_none() || *owner == Some(sched.current_tid())
        }
        Some(HandleObj::Semaphore { count, .. }) => *count > 0,
        Some(HandleObj::Thread { tid }) => sched
            .threads
            .iter()
            .any(|t| t.tid == *tid && matches!(t.state, ThreadState::Exited(_))),
        _ => false,
    }
}

/// Consuming acquire: events clear their signal (auto-reset), mutexes take
/// ownership, semaphores decrement. Thread handles have nothing to consume.
pub(crate) fn obj_acquire(handles: &mut KernelObjects32, sched: &Sched, h: u32) -> bool {
    match handles.get_mut(&h) {
        Some(HandleObj::Event { manual, signaled }) => {
            if *signaled {
                if !*manual {
                    *signaled = false;
                }
                true
            } else {
                false
            }
        }
        Some(HandleObj::Mutex { owner, count }) => {
            let me = sched.current_tid();
            match *owner {
                None => {
                    *owner = Some(me);
                    *count = 1;
                    true
                }
                Some(o) if o == me => {
                    *count += 1;
                    true
                }
                _ => false,
            }
        }
        Some(HandleObj::Semaphore { count, .. }) => {
            if *count > 0 {
                *count -= 1;
                true
            } else {
                false
            }
        }
        Some(HandleObj::Thread { tid }) => {
            let tid = *tid;
            sched
                .threads
                .iter()
                .any(|t| t.tid == tid && matches!(t.state, ThreadState::Exited(_)))
        }
        _ => false,
    }
}

/// Wake up to `max` threads waiting on object `h` so they re-test it.
pub(crate) fn wake_obj_waiters(sched: &mut Sched, h: u32, max: usize) -> usize {
    let mut n = 0;
    for i in 0..sched.threads.len() {
        if n >= max {
            break;
        }
        let hit = matches!(
            &sched.threads[i].state,
            ThreadState::Waiting { target, .. } if target_mentions(target, h)
        );
        if hit {
            sched.wake_thread(i, Wake::Retest);
            n += 1;
        }
    }
    n
}
pub(crate) fn signal_audio_event(handles: &mut KernelObjects32, sched: &mut Sched, h: u32) {
    if let Some(HandleObj::Event { manual, signaled }) = handles.get_mut(&h) {
        *signaled = true;
        let n = if *manual { usize::MAX } else { 1 };
        wake_obj_waiters(sched, h, n);
    }
}
