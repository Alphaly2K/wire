//! Cooperative guest-thread scheduler (M2).
//!
//! One host CPU runs all guest threads. Switch points: blocking API calls
//! (wait/sleep/critical-section contention), explicit yields (Sleep(0),
//! SwitchToThread), and a coarse instruction timeslice in the interpreter
//! loop so spin loops cannot starve other threads. Documented simplifications:
//! no true parallelism, no preemption visible inside a single instruction,
//! and waiters are re-tested on wake rather than competing fairly.

use crate::cpu::{Cpu, ThreadCtx, ESP};
use crate::mem::GuestMem;
use std::time::{Duration, Instant};

pub const WAIT_OBJECT_0: u32 = 0;
pub const WAIT_TIMEOUT: u32 = 0x102;

/// Worker thread stack: 1 MiB, committed at creation (host pages stay lazy).
/// The main thread's stack comes from the loader.
pub const THREAD_STACK_SIZE: u32 = 0x10_0000;
const STACK_ARENA_BASE: u32 = 0x0200_0000;
const TEB_ARENA_BASE: u32 = 0x7ffc_0000;
const MAX_THREADS: usize = 64;

/// Why a blocked thread was made runnable again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// Wait is over; value is the return code (WAIT_OBJECT_0+n / WAIT_TIMEOUT).
    Completed(u32),
    /// Object state changed; re-run the acquire test (may block again).
    Retest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitTarget {
    Object(u32),
    Multi(Vec<u32>, bool), // handles, wait_all
    CriticalSection(u32),  // guest CRITICAL_SECTION address
    Sleep,
}

pub fn target_mentions(t: &WaitTarget, h: u32) -> bool {
    match t {
        WaitTarget::Object(x) => *x == h,
        WaitTarget::Multi(v, _) => v.contains(&h),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThreadState {
    Runnable,
    Suspended,
    Waiting {
        target: WaitTarget,
        deadline: Option<Instant>,
    },
    Exited(u32),
}

pub struct GuestThread {
    pub tid: u32,
    pub handle: u32,
    pub stack_base: u32,
    pub teb: u32,
    /// Saved context. Valid only while this is not the current thread (the
    /// current thread's live context is in the Cpu).
    pub ctx: ThreadCtx,
    pub state: ThreadState,
    pub wake: Option<Wake>,
    /// Deadline of the wait the thread is currently in; survives Retest
    /// re-blocks so a re-blocked wait keeps its original timeout budget.
    pub wait_deadline: Option<Instant>,
}

pub struct Sched {
    pub threads: Vec<GuestThread>,
    pub current: usize,
    next_tid: u32,
}

impl Default for Sched {
    fn default() -> Sched {
        Sched::new()
    }
}

impl Sched {
    pub fn new() -> Sched {
        Sched {
            threads: Vec::new(),
            current: 0,
            next_tid: 1,
        }
    }

    pub fn alloc_tid(&mut self) -> u32 {
        let t = self.next_tid;
        self.next_tid += 1;
        t
    }

    /// Register the main thread (its live context already sits in the Cpu).
    pub fn add_main(&mut self, tid: u32, handle: u32, teb: u32) {
        self.threads.push(GuestThread {
            tid,
            handle,
            stack_base: crate::loader::STACK_BASE,
            teb,
            ctx: ThreadCtx::default(),
            state: ThreadState::Runnable,
            wake: None,
            wait_deadline: None,
        });
    }

    pub fn current_tid(&self) -> u32 {
        self.threads[self.current].tid
    }

    pub fn any_other_runnable(&self) -> bool {
        self.threads
            .iter()
            .enumerate()
            .any(|(i, t)| i != self.current && t.state == ThreadState::Runnable)
    }

    /// Timeslice / explicit yield: hand the CPU to the next runnable thread.
    /// Returns true if a switch happened.
    pub fn round_robin(&mut self, cpu: &mut Cpu) -> bool {
        let n = self.threads.len();
        for k in 1..n {
            let i = (self.current + k) % n;
            if self.threads[i].state == ThreadState::Runnable {
                self.switch_to(cpu, i);
                return true;
            }
        }
        false
    }

    fn switch_to(&mut self, cpu: &mut Cpu, next: usize) {
        if next == self.current {
            return;
        }
        let cur = self.current;
        self.threads[cur].ctx = cpu.save_ctx();
        self.current = next;
        let ctx = self.threads[next].ctx;
        cpu.load_ctx(&ctx);
    }

    /// Create a worker thread: guest stack + TEB + initial context whose
    /// "return address" is the wire::ExitThread stub.
    #[allow(clippy::too_many_arguments)]
    pub fn create_thread(
        &mut self,
        mem: &mut GuestMem,
        entry: u32,
        param: u32,
        suspended: bool,
        tid: u32,
        handle: u32,
        exit_thread_stub: u32,
    ) -> Result<(), String> {
        if self.threads.len() >= MAX_THREADS {
            return Err("guest thread cap reached".into());
        }
        let i = (self.threads.len() - 1) as u32; // slot 0 is the main thread
        let stack_base = STACK_ARENA_BASE + i * THREAD_STACK_SIZE;
        mem.commit(stack_base, THREAD_STACK_SIZE)
            .map_err(|e| format!("thread stack: {e}"))?;
        let teb = TEB_ARENA_BASE + i * 0x1000;
        mem.commit(teb, 0x1000).map_err(|e| format!("teb: {e}"))?;
        let stack_top = stack_base + THREAD_STACK_SIZE;
        mem.write_u32(teb, 0xffff_ffff).map_err(|e| e.to_string())?; // SEH chain: empty
        mem.write_u32(teb + 0x04, stack_top).map_err(|e| e.to_string())?;
        mem.write_u32(teb + 0x08, stack_base).map_err(|e| e.to_string())?;
        mem.write_u32(teb + 0x18, teb).map_err(|e| e.to_string())?; // Self
        mem.write_u32(teb + 0x20, 4242).map_err(|e| e.to_string())?; // pid
        mem.write_u32(teb + 0x24, tid).map_err(|e| e.to_string())?;
        mem.write_u32(teb + 0x30, crate::loader::PEB_ADDR)
            .map_err(|e| e.to_string())?;
        let esp = (stack_top - 16) & !15;
        mem.write_u32(esp, exit_thread_stub).map_err(|e| e.to_string())?;
        mem.write_u32(esp + 4, param).map_err(|e| e.to_string())?;
        let mut ctx = ThreadCtx {
            eip: entry,
            eflags: 0x202,
            fs_base: teb,
            ..ThreadCtx::default()
        };
        ctx.regs[ESP] = esp;
        self.threads.push(GuestThread {
            tid,
            handle,
            stack_base,
            teb,
            ctx,
            state: if suspended {
                ThreadState::Suspended
            } else {
                ThreadState::Runnable
            },
            wake: None,
            wait_deadline: None,
        });
        Ok(())
    }

    /// Mark the current thread waiting and run someone else. Control comes
    /// back to this thread later via re-dispatch of the same API call.
    pub fn block_current(
        &mut self,
        cpu: &mut Cpu,
        target: WaitTarget,
        deadline: Option<Instant>,
    ) -> Result<(), String> {
        let cur = self.current;
        self.threads[cur].state = ThreadState::Waiting { target, deadline };
        self.threads[cur].wake = None;
        self.switch_away(cpu)
    }

    /// Current thread exits; wake anyone waiting on its handle. Halts the
    /// machine when no thread is left to run.
    pub fn exit_current(&mut self, cpu: &mut Cpu, code: u32) -> Result<(), String> {
        let cur = self.current;
        self.threads[cur].state = ThreadState::Exited(code);
        let h = self.threads[cur].handle;
        for i in 0..self.threads.len() {
            if i == cur {
                continue;
            }
            let hit = matches!(
                &self.threads[i].state,
                ThreadState::Waiting { target, .. } if target_mentions(target, h)
            );
            if hit {
                self.wake_thread(i, Wake::Retest);
            }
        }
        self.switch_away(cpu)
    }

    fn switch_away(&mut self, cpu: &mut Cpu) -> Result<(), String> {
        loop {
            let now = Instant::now();
            for t in self.threads.iter_mut() {
                if let ThreadState::Waiting {
                    deadline: Some(d), ..
                } = t.state
                {
                    if d <= now {
                        t.state = ThreadState::Runnable;
                        t.wake = Some(Wake::Completed(WAIT_TIMEOUT));
                    }
                }
            }
            let n = self.threads.len();
            for k in 1..=n {
                let i = (self.current + k) % n;
                if self.threads[i].state == ThreadState::Runnable {
                    self.switch_to(cpu, i);
                    return Ok(());
                }
            }
            let nearest = self.threads.iter().filter_map(|t| match t.state {
                ThreadState::Waiting {
                    deadline: Some(d), ..
                } => Some(d),
                _ => None,
            });
            match nearest.min() {
                Some(d) => {
                    let dur = d
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(20));
                    std::thread::sleep(dur);
                }
                None => {
                    if self
                        .threads
                        .iter()
                        .any(|t| matches!(t.state, ThreadState::Waiting { .. } | ThreadState::Suspended))
                    {
                        return Err(self.describe_deadlock());
                    }
                    // Every guest thread has exited: end the process.
                    cpu.halted = true;
                    cpu.exit_code = Some(
                        self.threads
                            .iter()
                            .find_map(|t| match t.state {
                                ThreadState::Exited(c) => Some(c),
                                _ => None,
                            })
                            .unwrap_or(0),
                    );
                    return Ok(());
                }
            }
        }
    }

    fn describe_deadlock(&self) -> String {
        let mut s = String::from("guest deadlock: no runnable thread");
        for t in &self.threads {
            s.push_str(&format!("\n  tid {} handle {:#x}: {:?}", t.tid, t.handle, t.state));
        }
        s
    }

    pub fn wake_thread(&mut self, i: usize, w: Wake) {
        self.threads[i].state = ThreadState::Runnable;
        self.threads[i].wake = Some(w);
    }

    /// Consume the pending wake reason of the current thread. A Completed
    /// wake also clears the stored wait deadline (the wait is over); a
    /// Retest keeps it so a re-blocked wait keeps its timeout budget.
    pub fn take_wake(&mut self) -> Option<Wake> {
        let t = &mut self.threads[self.current];
        let w = t.wake.take();
        if matches!(w, Some(Wake::Completed(_))) {
            t.wait_deadline = None;
        }
        w
    }

    /// Arm (or keep) the deadline for the current thread's wait.
    pub fn arm_deadline(&mut self, timeout_ms: u32) -> Option<Instant> {
        let t = &mut self.threads[self.current];
        if t.wait_deadline.is_none() {
            t.wait_deadline = if timeout_ms == u32::MAX {
                None
            } else {
                Some(Instant::now() + Duration::from_millis(timeout_ms as u64))
            };
        }
        t.wait_deadline
    }

    pub fn clear_deadline(&mut self) {
        self.threads[self.current].wait_deadline = None;
    }

    pub fn thread_index_by_handle(&self, handle: u32) -> Option<usize> {
        self.threads.iter().position(|t| t.handle == handle)
    }
}
