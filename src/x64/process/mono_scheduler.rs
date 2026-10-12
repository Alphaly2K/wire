//! Scoped access to existing cooperative guest contexts while native Mono has
//! paused the main CPU. No second host thread executes guest code or aliases
//! Memory/Runtime. Unsupported worker transitions remain explicit failures.
use super::{Context, Memory, Runtime64, RETURN};
use crate::x64::runtime::Control;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ptr,
};

struct Scheduler<'a> {
    contexts: RefCell<&'a mut BTreeMap<u64, Context>>,
    loader_owner: Option<u64>,
    last: Cell<u64>,
    quanta: Cell<u64>,
}
thread_local! {
    static SCHEDULER: Cell<*const Scheduler<'static>> = const { Cell::new(ptr::null()) };
}
struct Scope;
impl Drop for Scope {
    fn drop(&mut self) {
        SCHEDULER.with(|slot| slot.set(ptr::null()));
    }
}
pub(super) fn with_scheduler<T>(
    contexts: &mut BTreeMap<u64, Context>,
    loader_owner: Option<u64>,
    operation: impl FnOnce() -> T,
) -> Result<T, String> {
    if SCHEDULER.with(|slot| !slot.get().is_null()) {
        return Err("native Mono cooperative scheduler already installed".into());
    }
    let scheduler = Scheduler {
        contexts: RefCell::new(contexts),
        loader_owner,
        last: Cell::new(1),
        quanta: Cell::new(0),
    };
    // Scope clears the lifetime-erased pointer before releasing the exclusive
    // contexts borrow, including when the native call unwinds with an error.
    SCHEDULER.with(|slot| slot.set((&scheduler as *const Scheduler<'_>).cast()));
    let _scope = Scope;
    Ok(operation())
}
pub(super) fn pump(
    memory: &mut Memory,
    runtime: &mut Runtime64,
    remaining: u64,
) -> Result<u64, String> {
    if remaining == 0 || runtime.active_thread() != 1 {
        return Err("native Mono wait pump requires bounded guest main owner".into());
    }
    SCHEDULER.with(|slot| {
        if slot.get().is_null() {
            return Err("native Mono wait has no cooperative process scheduler".into());
        }
        let scheduler = unsafe { &*slot.get() };
        let mut contexts = scheduler.contexts.try_borrow_mut()
            .map_err(|_| "native Mono cooperative contexts already borrowed")?;
        for spawn in runtime.take_threads() {
            let rsp = spawn.cpu.regs[4];
            let current = super::Call {
                name: format!("thread {} entry", spawn.id),
                target: spawn.cpu.rip,
                base: 0,
                notify: false,
                require_true: false,
                reason: 0,
                parameter: spawn.parameter,
            };
            contexts.insert(spawn.id, Context {
                cpu: spawn.cpu, current, calls: Default::default(), rsp,
                exit_code: None, started: false, callbacks: Vec::new(),
            });
        }
        let last = scheduler.last.get();
        let ids: Vec<_> = contexts.range((last + 1)..).chain(contexts.range(..=last))
            .map(|(&id, _)| id).filter(|&id| id != 1).collect();
        for id in ids {
            let context = &contexts[&id];
            if (!context.started || context.current.notify)
                && scheduler.loader_owner.is_some_and(|owner| owner != id) {
                continue;
            }
            if !runtime.thread_runnable(id, memory)? { continue; }
            if !context.started || context.current.notify || !context.callbacks.is_empty() {
                return Err(format!("native Mono wait requires worker notification/callback executor; guest-thread={id}; phase={}", context.current.name));
            }
            let mut context = contexts.remove(&id).unwrap();
            scheduler.last.set(id);
            runtime.activate_thread(id);
            let result = (|| {
                if context.cpu.gs_base != runtime.teb()
                    || memory.read_int(runtime.teb() + 0x48, 8)? != id {
                    return Err("native Mono wait worker TEB identity mismatch".into());
                }
                run_worker(&mut context, memory, runtime, remaining.min(10_000))
            })();
            runtime.activate_thread(1);
            contexts.insert(id, context);
            if let Ok(work) = &result {
                let quanta = scheduler.quanta.get() + 1;
                scheduler.quanta.set(quanta);
                if quanta <= 4 || quanta.is_power_of_two() {
                    eprintln!("NATIVE_MONO_COOPERATIVE_WAIT: worker={id}; quantum={quanta}; work={work}; main-runnable={}", runtime.thread_runnable(1, memory)?);
                }
            }
            return result;
        }
        if runtime.thread_runnable(1, memory)? { return Ok(1); }
        if let Some(delay) = runtime.idle_delay() {
            std::thread::sleep(delay);
            return Ok(1);
        }
        Err("native Mono guest wait deadlock: no runnable existing worker or timed wait".into())
    })
}
fn run_worker(
    context: &mut Context,
    memory: &mut Memory,
    runtime: &mut Runtime64,
    remaining: u64,
) -> Result<u64, String> {
    let start = context.cpu.work;
    while context.cpu.work - start < remaining {
        if context.cpu.rip == RETURN {
            // Returning may run FLS destructors and DLL_THREAD_DETACH. Keep the
            // context for the outer executor instead of skipping those calls.
            return Err(format!(
                "native Mono wait requires worker exit executor; guest-thread={}; phase={}",
                runtime.active_thread(),
                context.current.name
            ));
        }
        if let Some(binding) = runtime.gateways.binding(&context.cpu.rip) {
            if binding.dll.as_ref() == "mono-2.0-bdwgc.dll" {
                return Err(format!("native Mono wait requires typed worker reentry; guest-thread={}; function={}; args={:x?}", runtime.active_thread(), binding.original_function, [context.cpu.regs[1], context.cpu.regs[2], context.cpu.regs[8], context.cpu.regs[9]]));
            }
            let control = runtime.dispatch_bound(&binding, &mut context.cpu, memory)?;
            context.cpu.work += 1;
            match control {
                Control::Returned => {},
                Control::Yielded | Control::Blocked => break,
                other => return Err(format!("native Mono wait worker control not admitted; guest-thread={}; function={}!{}; control={other:?}", runtime.active_thread(), binding.dll, binding.original_function)),
            }
        } else {
            context
                .cpu
                .step_bounded(memory, remaining - (context.cpu.work - start))?;
        }
        // Resume the waiting main callback promptly after a real wake/timeout.
        if runtime.thread_runnable(1, memory)? {
            break;
        }
    }
    Ok(context.cpu.work - start)
}
