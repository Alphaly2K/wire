//! Same contracts as ordinary worker dispatch, also available while native
//! engine code has paused the guest main thread in a cooperative wait.
use super::*;

pub(in super::super) struct WorkerMetadata {
    plan: metadata::WorkerPlan,
    iterator_output: Option<(u64, u64)>,
}

impl<'a> Metadata<'a> {
    pub(in super::super) fn worker_state(
        &self,
    ) -> Result<&'a super::super::workers::State, String> {
        Ok(self
            .objects
            .as_ref()
            .ok_or("cooperative worker has no root route")?
            .workers)
    }
    pub(in super::super) fn prepare_worker_metadata(
        &mut self,
        function: &str,
        cpu: &Cpu64,
        memory: &Memory,
        runtime: &super::super::Runtime64,
    ) -> Result<(metadata::WorkerRequest, WorkerMetadata), String> {
        let iterator_output = if metadata::worker_iterates(function) {
            worker_output(memory, runtime, cpu, cpu.regs[2])?;
            if cpu.regs[2] == 0 {
                return Err("worker iterator output is NULL".into());
            }
            Some((cpu.regs[2], memory.read_int(cpu.regs[2], 8)?))
        } else {
            None
        };
        let (request, plan) = self.metadata.prepare_reentry_worker(
            self.library,
            function,
            [cpu.regs[1], cpu.regs[2], cpu.regs[8]],
            iterator_output.map(|(_, token)| token),
        )?;
        Ok((
            request,
            WorkerMetadata {
                plan,
                iterator_output,
            },
        ))
    }
    pub(in super::super) fn finish_worker_metadata(
        &mut self,
        prepared: WorkerMetadata,
        result: metadata::WorkerResult,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &super::super::Runtime64,
    ) -> Result<(), String> {
        if let Some((address, previous)) = prepared.iterator_output {
            worker_output(memory, runtime, cpu, address)?;
            if memory.read_int(address, 8)? != previous {
                return Err("worker iterator guest output changed during request".into());
            }
        }
        let (value, iterator) = self.metadata.finish_reentry_worker(prepared.plan, result)?;
        let class = matches!(value, metadata::Value::Handle(_, metadata::Kind::Class));
        let token = self.metadata.wrap(memory, value)?;
        if class && token != 0 {
            self.metadata
                .publish_class_elements(self.library, memory, token)?;
        }
        if let Some(iterator) = iterator {
            memory.write_int(
                prepared
                    .iterator_output
                    .ok_or("worker iterator output missing")?
                    .0,
                8,
                iterator,
            )?;
        }
        cpu.return_integer(memory, token)
    }
    pub(in super::super) fn prepare_worker_invoke(
        &mut self,
        cpu: &Cpu64,
        memory: &Memory,
        runtime: &super::super::Runtime64,
    ) -> Result<constructors::WorkerRequest, String> {
        worker_output(memory, runtime, cpu, cpu.regs[9])?;
        if cpu.regs[2] == 0 {
            return Err("worker instance entry requires rooted receiver".into());
        }
        let method = self.metadata.native(cpu.regs[1], metadata::Kind::Method)?;
        let mut images = self.images.live_native_images()?;
        let core = self.images.unique_native_image(super::super::CORE_MODULE)?;
        images.retain(|&image| image != core);
        images.insert(0, core);
        self.sync_guest(memory)?;
        let roots = self
            .objects
            .as_ref()
            .ok_or("worker invoke has no owned roots")?;
        let object = roots.roots.native(roots.api, cpu.regs[2])?;
        let class = roots.roots.class(roots.api, cpu.regs[2])?;
        eprintln!(
            "NATIVE_MONO_COOPERATIVE_INVOKE: guest-thread={}; {}",
            runtime.active_thread(),
            self.metadata.describe_method(self.library, cpu.regs[1])?
        );
        // The actual method proves void/no-argument instance IL. Any unused
        // caller scratch parameter vector remains private guest data.
        constructors::Api::load(self.library)?.prepare_worker(
            self.library,
            method,
            &images,
            object,
            class,
        )
    }
    pub(in super::super) fn finish_worker_invoke(
        &mut self,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &super::super::Runtime64,
    ) -> Result<(), String> {
        worker_output(memory, runtime, cpu, cpu.regs[9])?;
        self.refresh_native(memory)?;
        let roots = self
            .objects
            .as_mut()
            .ok_or("worker invoke completion has no roots")?;
        let token = cpu.regs[2];
        let object = roots.roots.native(roots.api, token)?;
        let class = roots.roots.class(roots.api, token)?;
        let images = self.images.live_native_images()?;
        let core = self.images.unique_native_image(super::super::CORE_MODULE)?;
        if !roots.fields.has_owner(token) {
            let specs = if roots.api.engine_class(class) {
                field_layouts::specs(self.library, core, &images, object, class)?
            } else {
                field_layouts::reference_storage_specs(self.library, core, &images, object, class)?
            };
            roots.fields.install_with(
                self.library,
                roots.api,
                roots.roots,
                memory,
                specs,
                |registry, native| {
                    roots
                        .objects
                        .borrow_reference(roots.strings, roots.api, registry, native)
                },
            )?;
        }
        let references = roots
            .fields
            .native_references(roots.api, roots.roots, token)?;
        roots.objects.publish_pending(memory)?;
        self.publish_container_graph(memory, references)?;
        worker_output(memory, runtime, cpu, cpu.regs[9])?;
        if cpu.regs[9] != 0 {
            memory.write_int(cpu.regs[9], 8, 0)?;
        }
        cpu.return_integer(memory, 0)
    }
}

fn worker_output(
    memory: &Memory,
    runtime: &super::super::Runtime64,
    cpu: &Cpu64,
    address: u64,
) -> Result<(), String> {
    let (base, end) = runtime.native_worker_stack_bounds(memory, cpu)?;
    memory.read_int(cpu.regs[4], 8)?;
    if address == 0 {
        return Ok(());
    }
    let stop = address
        .checked_add(8)
        .ok_or("worker exception slot overflow")?;
    let live = if address >= base && address < end {
        address >= cpu.regs[4].checked_add(8).ok_or("worker stack overflow")? && stop <= end
    } else {
        runtime.live_data_slot(address, 8) || memory.guest_private_data_slot(address, 8)
    };
    if !address.is_multiple_of(8) || !live || !memory.owned_noncode_access(address, 8) {
        return Err("worker exception output requires a live owned C++ slot".into());
    }
    memory.check(address, 8, crate::x64::memory::WRITE)
}
