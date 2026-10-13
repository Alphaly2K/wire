//! Two-phase cooperative worker service. Native requests run after releasing
//! the paused owner's VM/metadata borrows, on the already attached OS worker.
use super::super::super::mono_scheduler::{self, Pump};
use super::*;

pub(super) fn pump(owner: &Owner<'_>, remaining: u64) -> Result<u64, String> {
    let progress = mono_scheduler::pump(
        &mut owner.memory.borrow_mut(),
        &mut owner.runtime.borrow_mut(),
        remaining,
    )?;
    let Pump::Reentry {
        guest,
        target,
        function,
        work,
    } = progress
    else {
        let Pump::Advanced(work) = progress else {
            unreachable!()
        };
        return Ok(work);
    };
    if work >= remaining {
        return Err("worker reentry exceeds callback work budget".into());
    }
    owner.runtime.borrow_mut().activate_thread(guest);
    struct Active<'a, 'b>(&'a Owner<'b>);
    impl Drop for Active<'_, '_> {
        fn drop(&mut self) {
            self.0.runtime.borrow_mut().activate_thread(1);
        }
    }
    let _active = Active(owner);
    mono_scheduler::with_reentry(guest, target, |cpu| dispatch(owner, &function, cpu))?;
    Ok(work + 1)
}

fn dispatch(owner: &Owner<'_>, function: &str, cpu: &mut Cpu64) -> Result<(), String> {
    let admitted_leaf = matches!(
        function,
        "mono_object_get_class"
            | "mono_object_unbox"
            | "mono_array_addr_with_size"
            | "mono_array_length"
            | "mono_string_length"
            | "mono_string_chars"
            | "mono_field_get_value"
            | "mono_field_set_value"
            | "mono_gc_wbarrier_set_field"
            | "mono_gc_wbarrier_set_arrayref"
            | "mono_gchandle_new_v2"
            | "mono_gchandle_new_weakref_v2"
            | "mono_gchandle_get_target_v2"
            | "mono_gchandle_free_v2"
            | "mono_class_get_userdata"
            | "mono_class_set_userdata"
            | "mono_get_corlib"
            | "mono_class_get_image"
            | "mono_image_get_name"
            | "mono_image_get_table_rows"
    ) || super::super::metadata::thread_neutral_leaf(function);
    if !admitted_leaf
        && !super::super::metadata::worker_handles(function)
        && !super::super::reentry::handles_allocation(function)
        && !matches!(
            function,
            "mono_runtime_invoke" | "mono_object_get_virtual_method"
        )
    {
        return Err(format!("native Mono wait requires typed worker reentry; guest-thread={}; function={function}; args={:x?}",
            owner.runtime.borrow().active_thread(), [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]]));
    }
    let route = owner
        .metadata
        .as_ref()
        .ok_or("cooperative worker has no root route")?;
    let (workers, domain) = {
        let route = route
            .try_borrow()
            .map_err(|_| "worker reentry metadata is already borrowed")?;
        (route.worker_state()?, route.domain)
    };
    let guest = owner.runtime.borrow().active_thread();
    workers.observed_lifecycle(domain, guest, "mono_thread_current")?;
    let stack = owner
        .runtime
        .borrow()
        .native_worker_stack_bounds(&owner.memory.borrow(), cpu)?;
    if super::super::metadata::worker_handles(function) && !admitted_leaf {
        let (request, prepared) = route
            .try_borrow_mut()
            .map_err(|_| "worker query metadata is already borrowed")?
            .prepare_worker_metadata(
                function,
                cpu,
                &owner.memory.borrow(),
                &owner.runtime.borrow(),
            )?;
        let result = workers.metadata(guest, request)?;
        route
            .try_borrow_mut()
            .map_err(|_| "worker query completion metadata is already borrowed")?
            .finish_worker_metadata(
                prepared,
                result,
                cpu,
                &mut owner.memory.borrow_mut(),
                &owner.runtime.borrow(),
            )?;
    } else if function == "mono_runtime_invoke" {
        let request = route
            .try_borrow_mut()
            .map_err(|_| "worker invoke metadata is already borrowed")?
            .prepare_worker_invoke(cpu, &owner.memory.borrow(), &owner.runtime.borrow())?;
        workers.invoke(guest, request)?;
        route
            .try_borrow_mut()
            .map_err(|_| "worker invoke completion metadata is already borrowed")?
            .finish_worker_invoke(cpu, &mut owner.memory.borrow_mut(), &owner.runtime.borrow())?;
        owner
            .managed_reentries
            .set(owner.managed_reentries.get() + 1);
    } else if function == "mono_object_get_virtual_method" {
        let request = route
            .try_borrow_mut()
            .map_err(|_| "worker virtual metadata is already borrowed")?
            .prepare_virtual(cpu, &owner.memory.borrow())?;
        let resolved = workers.resolve_virtual(guest, request)?;
        let token = route
            .try_borrow_mut()
            .map_err(|_| "worker virtual completion metadata is already borrowed")?
            .finish_virtual(resolved, &mut owner.memory.borrow_mut())?;
        cpu.return_integer(&owner.memory.borrow(), token)?;
    } else if super::super::reentry::handles_allocation(function) {
        let request = route
            .try_borrow_mut()
            .map_err(|_| "worker allocation metadata is already borrowed")?
            .prepare_allocation(function, cpu, &owner.memory.borrow())?;
        let allocated = workers.allocate_reentry(guest, request)?;
        let token = route
            .try_borrow_mut()
            .map_err(|_| "worker allocation completion metadata is already borrowed")?
            .finish_allocation(allocated, &mut owner.memory.borrow_mut())?;
        cpu.return_integer(&owner.memory.borrow(), token)?;
    } else {
        // Frozen-source v2 handles and rooted views use process-wide locked
        // tables, carry their own domain and do not invoke managed IL/change
        // worker TLS. Keep the same checked identity/typed barrier protocol.
        route
            .try_borrow_mut()
            .map_err(|_| "worker leaf metadata is already borrowed")?
            .dispatch(function, cpu, &mut owner.memory.borrow_mut(), stack)?;
    }
    owner.mono_reentries.set(owner.mono_reentries.get() + 1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::{
        memory::{READ, WRITE},
        process::{Call, Context, RETURN, STACK},
    };
    use std::collections::BTreeMap;

    #[test]
    fn worker_gateway_yields_vm_borrows_and_keeps_exact_context_on_failure() {
        let mut memory = Memory::default();
        memory.map(STACK, 65536, READ | WRITE).unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            STACK,
            65536,
            "worker-reentry-context",
        )
        .unwrap();
        runtime.enable_scheduler();
        let target = runtime.register("mono-2.0-bdwgc.dll", "mono_runtime_invoke");
        let rsp = STACK + 65536 - 72;
        memory.write_int(rsp, 8, RETURN).unwrap();
        memory.write_int(rsp + 40, 8, 0).unwrap();
        memory.write_int(rsp + 48, 8, 0).unwrap();
        let mut cpu = Cpu64::new(0, rsp);
        cpu.regs[8] = target;
        runtime
            .dispatch("kernel32.dll", "CreateThread", &mut cpu, &mut memory)
            .unwrap();
        let spawn = runtime.take_threads().pop().unwrap();
        let guest = spawn.id;
        let mut contexts = BTreeMap::from([(
            guest,
            Context {
                rsp: spawn.cpu.regs[4],
                cpu: spawn.cpu,
                current: Call {
                    name: "started worker".into(),
                    target,
                    base: 0,
                    notify: false,
                    require_true: false,
                    reason: 0,
                    parameter: 0,
                },
                calls: Default::default(),
                exit_code: None,
                started: true,
                callbacks: Vec::new(),
            },
        )]);
        let (result, _) = mono_scheduler::with_scheduler(&mut contexts, None, || {
            with_owner(&mut memory, &mut runtime, [0; 7], || {
                let progress = with_service(|memory, runtime| mono_scheduler::pump(memory, runtime, 100))?;
                assert!(matches!(progress, Pump::Reentry { guest: actual, target: pc, ref function, work: 0 }
                    if actual == guest && pc == target && function == "mono_runtime_invoke"));
                // Both VM RefCell borrows from pump have ended. Failed service
                // does not consume the gateway or remove its CPU/context.
                let failed = mono_scheduler::with_reentry(guest, target, |cpu| {
                    with_service(|memory, runtime| {
                        assert_eq!(runtime.active_thread(), 1);
                        assert_eq!(cpu.rip, target);
                        memory.read_int(cpu.regs[4], 8)?;
                        Ok(())
                    })?;
                    Err::<(), _>("injected service failure".into())
                });
                assert!(matches!(failed, Err(error) if error == "injected service failure"));
                assert!(mono_scheduler::with_reentry(guest, target + 8, |_| Ok(())).is_err());
                mono_scheduler::with_reentry(guest, target, |cpu| with_service(|memory, _| {
                    assert_eq!(cpu.work, 0);
                    cpu.return_integer(memory, 0xabc)
                }))
            })
        }).unwrap().unwrap();
        result.unwrap();
        assert_eq!(runtime.active_thread(), 1);
        assert_eq!(contexts[&guest].cpu.work, 1);
        assert_ne!(contexts[&guest].cpu.rip, target);
        assert_eq!(contexts[&guest].cpu.regs[0], 0xabc);
    }
}
