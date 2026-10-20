//! PE32 blocking and wake coordination. The API facade completes return frames.
use crate::{
    api::{ApiLogger, Args, KernelObjects32},
    cpu::Cpu,
    mem::GuestMem,
    thread::{Sched, ThreadState, WaitTarget, Wake, WAIT_OBJECT_0, WAIT_TIMEOUT},
    user32::User32,
    win32::{
        catalog::{CriticalSectionApi, DelayApi, MessageApi, WaitApi},
        x86::object_waits::{obj_acquire, obj_ready},
    },
};
use std::time::Instant;

pub(crate) enum Outcome {
    Returned {
        value: u32,
        note: String,
        yield_after_return: bool,
    },
    Blocked,
}

pub(crate) fn critical_section(
    id: CriticalSectionApi,
    args: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    log: &mut ApiLogger,
    sched: &mut Sched,
) -> Result<Outcome, String> {
    use crate::win32::x86::critical_sections::{call, Outcome as Request};
    let ret = match call(id, args, mem, sched.current_tid())? {
        Request::Value(value) => value,
        Request::Block { address: cs, owner } => {
            sched.take_wake();
            log.note(&format!(
                "thread {} blocks on CRITICAL_SECTION {cs:#x} (owner tid {owner})",
                sched.current_tid()
            ));
            sched.block_current(cpu, WaitTarget::CriticalSection(cs), None)?;
            return Ok(Outcome::Blocked);
        }
        Request::WakeOne(cs) => {
            for i in 0..sched.threads.len() {
                if matches!(&sched.threads[i].state, ThreadState::Waiting {
                            target: WaitTarget::CriticalSection(a), ..
                        } if *a == cs)
                {
                    sched.wake_thread(i, Wake::Retest);
                    break;
                }
            }
            0
        }
    };
    Ok(Outcome::Returned {
        value: ret,
        note: String::new(),
        yield_after_return: false,
    })
}

pub(crate) fn delay(
    id: DelayApi,
    args: &Args,
    cpu: &mut Cpu,
    log: &mut ApiLogger,
    sched: &mut Sched,
) -> Result<Outcome, String> {
    let ret;
    let mut yield_after_return = false;
    match id {
        DelayApi::SwitchThread => {
            sched.poll_timeouts();
            ret = sched.any_other_runnable() as u32;
            yield_after_return = ret != 0;
        }
        DelayApi::Sleep => {
            if sched.take_wake().is_some() {
                ret = 0; // void; woke after the full delay
            } else if args[0] == 0 {
                yield_after_return = true;
                ret = 0;
            } else {
                let d = sched.arm_deadline(args[0]);
                log.note(&format!(
                    "thread {} sleeps {}ms",
                    sched.current_tid(),
                    args[0]
                ));
                sched.block_current(cpu, WaitTarget::Sleep, d)?;
                return Ok(Outcome::Blocked);
            }
        }
        DelayApi::SleepEx => {
            if sched.take_wake().is_some() {
                ret = 0;
            } else if args[0] == 0 {
                yield_after_return = true;
                ret = 0;
            } else {
                let d = sched.arm_deadline(args[0]);
                sched.block_current(cpu, WaitTarget::Sleep, d)?;
                return Ok(Outcome::Blocked);
            }
        }
    }
    Ok(Outcome::Returned {
        value: ret,
        note: String::new(),
        yield_after_return,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wait(
    id: WaitApi,
    args: &Args,
    cpu: &mut Cpu,
    mem: &GuestMem,
    objects: &mut KernelObjects32,
    last_error: &mut u32,
    log: &mut ApiLogger,
    sched: &mut Sched,
) -> Result<Outcome, String> {
    let ret;
    let mut note = String::new();
    match id {
        WaitApi::Single | WaitApi::SingleEx => {
            let (h, timeout) = (args[0], args[1]);
            match sched.take_wake() {
                Some(Wake::Completed(r)) => {
                    ret = r;
                    note = "wait completed".into();
                }
                wake => {
                    if obj_acquire(objects, sched, h) {
                        sched.clear_deadline();
                        ret = WAIT_OBJECT_0;
                    } else if !objects.contains_key(&h) {
                        sched.clear_deadline();
                        *last_error = 6;
                        ret = 0xffff_ffff; // WAIT_FAILED
                        note = "invalid handle".into();
                    } else if timeout == 0 && wake.is_none() {
                        ret = WAIT_TIMEOUT;
                        note = "poll, not ready".into();
                    } else {
                        let d = sched.arm_deadline(timeout);
                        log.note(&format!(
                            "thread {} blocks in WaitForSingleObject({h:#x}, {}ms)",
                            sched.current_tid(),
                            if timeout == u32::MAX {
                                "inf".into()
                            } else {
                                timeout.to_string()
                            }
                        ));
                        sched.block_current(cpu, WaitTarget::Object(h), d)?;
                        return Ok(Outcome::Blocked);
                    }
                }
            }
        }
        WaitApi::Multiple => {
            let (n, ptr, wait_all, timeout) = (args[0], args[1], args[2] != 0, args[3]);
            let mut hs = Vec::new();
            for i in 0..n.min(64) {
                hs.push(mem.read_u32(ptr + 4 * i).map_err(|e| e.to_string())?);
            }
            match sched.take_wake() {
                Some(Wake::Completed(r)) => {
                    ret = r;
                    note = "wait completed".into();
                }
                wake => {
                    let mut got = None;
                    if wait_all {
                        if hs.iter().all(|&h| obj_ready(objects, sched, h)) {
                            for &h in &hs {
                                obj_acquire(objects, sched, h);
                            }
                            got = Some(0);
                        }
                    } else {
                        for (i, &h) in hs.iter().enumerate() {
                            if obj_acquire(objects, sched, h) {
                                got = Some(i as u32);
                                break;
                            }
                        }
                    }
                    match got {
                        Some(i) => {
                            sched.clear_deadline();
                            ret = WAIT_OBJECT_0 + i;
                        }
                        None if timeout == 0 && wake.is_none() => {
                            ret = WAIT_TIMEOUT;
                            note = "poll, not ready".into();
                        }
                        None => {
                            let d = sched.arm_deadline(timeout);
                            log.note(&format!(
                                "thread {} blocks in WaitForMultipleObjects({hs:?}, all={wait_all})",
                                sched.current_tid()
                            ));
                            sched.block_current(cpu, WaitTarget::Multi(hs, wait_all), d)?;
                            return Ok(Outcome::Blocked);
                        }
                    }
                }
            }
        }
        WaitApi::MultipleEx => unreachable!("PE32 extended multiple wait remains ABI-only"),
    }
    Ok(Outcome::Returned {
        value: ret,
        note,
        yield_after_return: false,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn message_wait(
    id: MessageApi,
    args: &Args,
    cpu: &mut Cpu,
    mem: &GuestMem,
    objects: &mut KernelObjects32,
    user: &mut User32,
    start: Instant,
    last_error: &mut u32,
    sched: &mut Sched,
) -> Result<Outcome, String> {
    let ret;
    let mut note = String::new();
    let (n, ptr, timeout, mask, flags) = if id == MessageApi::WaitObjectsEx {
        (args[0], args[1], args[2], args[3], args[4])
    } else {
        (
            args[0],
            args[1],
            args[3],
            args[4],
            if args[2] != 0 { 1 } else { 0 },
        )
    };
    sched.take_wake();
    if n > 63 || flags & !7 != 0 || flags & 2 != 0 || mask & !0x5ff != 0 {
        sched.clear_deadline();
        *last_error = 87;
        ret = u32::MAX;
    } else {
        let hs = (0..n)
            .map(|i| mem.read_u32(ptr + i * 4).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let tid = sched.current_tid();
        let input = user.message_wait_ready(start, tid, mask, flags & 4 != 0);
        let invalid = hs.iter().any(|h| !objects.contains_key(h));
        let got = if invalid {
            None
        } else if flags & 1 != 0 {
            if input && hs.iter().all(|&h| obj_ready(objects, sched, h)) {
                for &h in &hs {
                    obj_acquire(objects, sched, h);
                }
                Some(0)
            } else {
                None
            }
        } else {
            hs.iter()
                .enumerate()
                .find_map(|(i, &h)| obj_acquire(objects, sched, h).then_some(i as u32))
                .or(input.then_some(n))
        };
        if invalid {
            sched.clear_deadline();
            *last_error = 6;
            ret = u32::MAX;
        } else if let Some(r) = got {
            sched.clear_deadline();
            ret = r;
        } else {
            let deadline = sched.arm_deadline(timeout);
            if timeout == 0 || deadline.is_some_and(|d| d <= Instant::now()) {
                sched.clear_deadline();
                ret = WAIT_TIMEOUT;
            } else {
                let timer = user
                    .timers
                    .iter()
                    .filter(|((owner, _, _), _)| *owner == tid && mask & 0x10 != 0)
                    .map(|(_, t)| t.deadline)
                    .min();
                let wake = match (deadline, timer) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                sched.block_current(cpu, WaitTarget::MessageObjects(hs, tid, mask), wake)?;
                return Ok(Outcome::Blocked);
            }
        }
        note = "checked object/message wait".into();
    }
    Ok(Outcome::Returned {
        value: ret,
        note,
        yield_after_return: false,
    })
}
