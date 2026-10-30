use super::*;
use crate::win32::{call::Call, context};

pub(crate) enum LoadError {
    Fault(Fault),
    Windows(u32),
}
impl From<Fault> for LoadError {
    fn from(value: Fault) -> Self {
        Self::Fault(value)
    }
}
impl From<linker::Error> for LoadError {
    fn from(value: linker::Error) -> Self {
        Self::Windows(value.code)
    }
}

pub(super) enum Finish {
    Api(Call),
    Reject,
    Resume,
    Resolve(super::symbols::Resolve),
    Thread { exit: Option<u32> },
    ExitThread(u32),
    ExitProcess,
}

pub(super) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) finish: Finish,
    queue: VecDeque<Initializer>,
    current: Option<Initializer>,
    pub(super) created: BTreeSet<u64>,
    ready: Vec<u64>,
    edges: Vec<(u64, u64)>,
    remove: BTreeSet<u64>,
    result: u64,
    error: Option<u32>,
    sp: u64,
}

fn frame_sp(state: &State, mode: Mode) -> Result<u64, Fault> {
    if mode == Mode::X86 {
        return Ok(mode.address(state.regs[4].wrapping_sub(16)));
    }
    (state.regs[4] & !15).checked_sub(40).ok_or(Fault {
        address: state.regs[4],
        length: 8,
        access: WRITE,
        kind: crate::memory::FaultKind::Address,
    })
}

fn frame_bytes(mode: Mode, notify: &Initializer) -> Vec<u8> {
    let width = mode as usize / 8;
    let mut bytes = gateways::layout(mode).callback.to_le_bytes()[..width].to_vec();
    if mode == Mode::X86 {
        for value in [notify.base, notify.reason, notify.reserved] {
            bytes.extend_from_slice(&(value as u32).to_le_bytes());
        }
    }
    bytes
}

impl Loaded {
    fn garbage(&self, release: Option<(u64, u32)>) -> BTreeSet<u64> {
        let mut pending: Vec<_> = self
            .modules
            .modules
            .values()
            .filter(|m| {
                let refs = release
                    .filter(|(base, _)| *base == m.base)
                    .map_or(m.references, |(_, refs)| refs);
                m.pinned || refs != 0
            })
            .map(|m| m.base)
            .collect();
        // Initializers retain their images even before all external calls return.
        pending.extend(self.frames.iter().flat_map(|f| f.created.iter()).copied());
        let mut live = BTreeSet::new();
        while let Some(base) = pending.pop() {
            if !live.insert(base) {
                continue;
            }
            if let Some(module) = self.modules.modules.get(&base) {
                pending.extend(&module.dependencies);
            }
        }
        self.modules
            .modules
            .keys()
            .filter(|base| !live.contains(base))
            .copied()
            .collect()
    }

    fn detach_queue_with(&self, selected: &BTreeSet<u64>, reserved: u64) -> VecDeque<Initializer> {
        let mut reach = std::collections::BTreeMap::new();
        for &base in selected {
            let mut seen = BTreeSet::new();
            let mut pending = vec![base];
            while let Some(next) = pending.pop() {
                if !seen.insert(next) {
                    continue;
                }
                pending.extend(
                    self.modules.modules[&next]
                        .dependencies
                        .iter()
                        .filter(|base| selected.contains(base))
                        .copied(),
                );
            }
            reach.insert(base, seen);
        }
        let mut groups: Vec<Vec<u64>> = Vec::new();
        let mut remaining = selected.clone();
        while let Some(&base) = remaining.first() {
            let group: Vec<_> = remaining
                .iter()
                .filter(|&&to| reach[&base].contains(&to) && reach[&to].contains(&base))
                .copied()
                .collect();
            for base in &group {
                remaining.remove(base);
            }
            groups.push(group);
        }
        let group_of: std::collections::BTreeMap<_, _> = groups
            .iter()
            .enumerate()
            .flat_map(|(index, bases)| bases.iter().map(move |&base| (base, index)))
            .collect();
        let rank: std::collections::BTreeMap<_, _> = self
            .initialized
            .iter()
            .enumerate()
            .map(|(rank, &base)| (base, rank))
            .collect();
        let mut groups_left: BTreeSet<_> = (0..groups.len()).collect();
        let mut queue = VecDeque::new();
        while !groups_left.is_empty() {
            // A forwarder discovered after initialization still detaches before
            // its dependencies. Cycles retain reverse initialization order.
            let next = groups_left
                .iter()
                .filter(|&&to| {
                    !groups_left.iter().any(|&from| {
                        from != to
                            && groups[from].iter().any(|base| {
                                self.modules.modules[base]
                                    .dependencies
                                    .iter()
                                    .any(|dep| group_of.get(dep) == Some(&to))
                            })
                    })
                })
                .max_by_key(|&&group| {
                    groups[group]
                        .iter()
                        .filter_map(|base| rank.get(base))
                        .max()
                        .copied()
                        .unwrap_or(0)
                })
                .copied()
                .unwrap();
            groups_left.remove(&next);
            let mut members = groups[next].clone();
            members.sort_by_key(|base| std::cmp::Reverse(rank.get(base).copied().unwrap_or(0)));
            for base in members {
                if rank.contains_key(&base) {
                    queue.extend(notifications(&self.modules.modules[&base], 0, reserved));
                }
            }
        }
        queue
    }
    fn detach_queue(&self, selected: &BTreeSet<u64>) -> VecDeque<Initializer> {
        self.detach_queue_with(selected, 0)
    }
    pub(super) fn process_detach(
        &mut self,
        state: &mut State,
        memory: &Memory,
    ) -> Result<(), StartupError> {
        let selected: BTreeSet<_> = self.initialized.iter().copied().collect();
        let queue = self.detach_queue_with(&selected, 1);
        let sp = self.preflight_frame(&queue, state, memory)?;
        for base in &selected {
            self.modules.modules.get_mut(base).unwrap().phase = Phase::Detaching;
        }
        self.frames.push(Frame {
            saved: Box::new(state.clone()),
            finish: Finish::ExitProcess,
            queue,
            current: None,
            created: selected,
            ready: Vec::new(),
            edges: Vec::new(),
            remove: BTreeSet::new(),
            result: 0,
            error: None,
            sp,
        });
        state.ip = gateways::layout(memory.mode()).callback;
        Ok(())
    }

    fn preflight_frame(
        &self,
        queue: &VecDeque<Initializer>,
        state: &State,
        memory: &Memory,
    ) -> Result<u64, Fault> {
        if queue.is_empty() {
            return Ok(memory.mode().address(state.regs[4]));
        }
        let sp = frame_sp(state, memory.mode())?;
        if let Some(notify) = queue.front() {
            memory.check(sp, frame_bytes(memory.mode(), notify).len(), WRITE)?;
        }
        Ok(sp)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn load_library(
        &mut self,
        name: &str,
        flags: u32,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &mut Services,
        files: &Files,
    ) -> Result<(), LoadError> {
        self.start_load_with(
            name,
            flags,
            Finish::Api(call),
            state,
            memory,
            services,
            files,
        )
    }

    pub(super) fn start_load(
        &mut self,
        name: &str,
        finish: Finish,
        state: &mut State,
        memory: &mut Memory,
        services: &mut Services,
        files: &Files,
    ) -> Result<(), LoadError> {
        self.start_load_with(name, 0, finish, state, memory, services, files)
    }

    #[allow(clippy::too_many_arguments)]
    fn start_load_with(
        &mut self,
        name: &str,
        flags: u32,
        finish: Finish,
        state: &mut State,
        memory: &mut Memory,
        services: &mut Services,
        files: &Files,
    ) -> Result<(), LoadError> {
        if self.rejected {
            return Err(LoadError::Windows(1114));
        }
        let name = crate::pe::search::filename(name).map_err(LoadError::Windows)?;
        let base_name = name.rsplit('\\').next().unwrap().to_ascii_lowercase();
        if linker::provider(&base_name, memory.mode()) {
            let name = self
                .search
                .provider(&name, files)
                .map_err(LoadError::Windows)?
                .ok_or(LoadError::Windows(126))?;
            let handle = services
                .register_provider(&name)
                .map_err(|_| LoadError::Windows(8))?;
            self.finish_load(finish, handle, state, memory.mode());
            return Ok(());
        }
        let existing = self.modules.base_at(&name, files.cwd());
        if existing != 0 {
            let module = self.modules.modules.get_mut(&existing).unwrap();
            if matches!(
                module.phase,
                Phase::Mapped | Phase::Detaching | Phase::Failed
            ) {
                return Err(LoadError::Windows(1114));
            }
            module.references = module
                .references
                .checked_add(1)
                .ok_or(LoadError::Windows(8))?;
            self.finish_load(finish, existing, state, memory.mode());
            return Ok(());
        }
        if self.frames.len() >= 32 {
            return Err(LoadError::Windows(8));
        }
        let mut staged = Memory::new(memory.mode());
        staged.set_dep_enabled(memory.dep_enabled());
        let mut bindings = services.clone();
        let (mut linked, edges) = linker::extend(
            memory,
            &mut staged,
            files,
            &mut bindings,
            &self.modules,
            &name,
            self.delays.remaining(),
            &self.search,
            flags,
            &self.open_files,
            self.diagnostic_imports,
        )?;
        self.initialize_crt_data(&mut bindings, &mut staged);
        let delays = self
            .delays
            .stage(&mut linked, &mut staged)
            .map_err(|_| LoadError::Windows(193))?;
        let root = linked.root;
        linked.modules.get_mut(&root).unwrap().references = 1;
        let mut slots: BTreeSet<_> = self
            .modules
            .modules
            .values()
            .filter_map(|m| m.tls.as_ref().map(|t| t.slot))
            .collect();
        let mut writes = Vec::new();
        let mut queue = VecDeque::new();
        let mut ready = Vec::new();
        for &base in &linked.order {
            let module = linked.modules.get_mut(&base).unwrap();
            let slot = (0..64)
                .find(|slot| !slots.contains(slot))
                .ok_or(LoadError::Windows(8))?;
            let live: Vec<_> = self.threads.live().collect();
            module.tls = prepare_tls(
                &mut staged,
                memory,
                module,
                slot,
                live.first().ok_or(LoadError::Windows(6))?.0,
            )
            .map_err(|_| LoadError::Windows(193))?;
            if let Some(tls) = &mut module.tls {
                for (id, array) in live {
                    if !tls.blocks.contains_key(&id) {
                        let block =
                            allocate_process_data(&mut staged, memory, tls.size, &tls.template)
                                .map_err(|_| LoadError::Windows(8))?;
                        tls.blocks.insert(id, block);
                    }
                    let address = array + tls.slot as u64 * (memory.mode() as u64 / 8);
                    memory.check(address, memory.mode() as usize / 8, WRITE)?;
                    writes.push((address, tls.blocks[&id]));
                }
                slots.insert(slot);
            }
            let calls = notifications(module, 1, 0);
            if calls.is_empty() {
                ready.push(base);
            }
            module.phase = Phase::Initializing;
            queue.extend(calls);
        }
        let sp = self.preflight_frame(&queue, state, memory)?;
        let created = linked.modules.keys().copied().collect();
        let saved = Box::new(state.clone());
        memory.merge(staged).map_err(|_| LoadError::Windows(8))?;
        for (at, value) in writes {
            memory.write_int(at, memory.mode() as usize / 8, value)?;
        }
        self.modules.modules.extend(linked.modules);
        self.delays.commit(delays);
        for &(from, to) in &edges {
            self.modules
                .modules
                .get_mut(&from)
                .unwrap()
                .dependencies
                .push(to);
        }
        *services = bindings;
        services.own_crt_data(&mut self.data_pages);
        if matches!(finish, Finish::Resume) {
            self.resolution_loading(root);
        }
        self.frames.push(Frame {
            saved,
            finish,
            queue,
            current: None,
            created,
            ready,
            edges,
            remove: BTreeSet::new(),
            result: root,
            error: None,
            sp,
        });
        state.ip = gateways::layout(memory.mode()).callback;
        Ok(())
    }

    pub(crate) fn free_library(
        &mut self,
        base: u64,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), LoadError> {
        self.start_free(base, Finish::Api(call), state, memory, services)
    }

    pub(crate) fn free_and_exit_thread(
        &mut self,
        base: u64,
        code: u32,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), LoadError> {
        if !self.can_exit_thread() {
            return Err(LoadError::Windows(170));
        }
        match self.start_free(base, Finish::ExitThread(code), state, memory, services) {
            Err(LoadError::Windows(error)) => {
                memory.write_int(
                    context::error_address(state, memory.mode()),
                    4,
                    error.into(),
                )?;
                self.exit_thread(code, state, memory.mode());
                Ok(())
            }
            result => result,
        }
    }

    fn start_free(
        &mut self,
        base: u64,
        finish: Finish,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), LoadError> {
        if services.provider_name(base).is_some() {
            self.finish_free(finish, state, memory.mode());
            return Ok(());
        }
        let module = self
            .modules
            .modules
            .get(&base)
            .ok_or(LoadError::Windows(6))?;
        if matches!(module.phase, Phase::Initializing | Phase::Detaching) {
            return Err(LoadError::Windows(170));
        }
        let refs = module.references.saturating_sub(1);
        let remove = self.garbage(Some((base, refs)));
        if !remove.is_empty() && self.frames.len() >= 32 {
            return Err(LoadError::Windows(8));
        }
        let queue = self.detach_queue(&remove);
        let sp = self.preflight_frame(&queue, state, memory)?;
        self.modules.modules.get_mut(&base).unwrap().references = refs;
        if remove.is_empty() {
            self.finish_free(finish, state, memory.mode());
            return Ok(());
        }
        for base in &remove {
            self.modules.modules.get_mut(base).unwrap().phase = Phase::Detaching;
        }
        self.frames.push(Frame {
            saved: Box::new(state.clone()),
            finish,
            queue,
            current: None,
            created: BTreeSet::new(),
            ready: Vec::new(),
            edges: Vec::new(),
            remove,
            result: 1,
            error: None,
            sp,
        });
        state.ip = gateways::layout(memory.mode()).callback;
        Ok(())
    }

    pub(super) fn reject_static(&mut self, state: &State) {
        let failed = self
            .completed
            .take()
            .expect("failed static DllMain has a completion owner");
        self.modules.modules.get_mut(&failed).unwrap().phase = Phase::Failed;
        self.initialized.push(failed);
        self.rejected = true;
        self.require_true = false;
        self.initializers.clear();
        let remove = self.modules.modules.keys().copied().collect();
        let queue = self.detach_queue(&remove);
        for module in self.modules.modules.values_mut() {
            module.phase = Phase::Detaching;
        }
        self.frames.push(Frame {
            saved: Box::new(state.clone()),
            finish: Finish::Reject,
            queue,
            current: None,
            created: BTreeSet::new(),
            ready: Vec::new(),
            edges: Vec::new(),
            remove,
            result: 0,
            error: Some(1114),
            sp: self.sp,
        });
    }

    fn rollback(&mut self, frame: &mut Frame, failed: u64) {
        for &(from, to) in &frame.edges {
            if let Some(module) = self.modules.modules.get_mut(&from) {
                module.dependencies.retain(|base| *base != to);
            }
        }
        let root = self.modules.modules.get_mut(&frame.result).unwrap();
        root.references = root.references.saturating_sub(1);
        for base in &frame.created {
            let module = self.modules.modules.get_mut(base).unwrap();
            if module.phase == Phase::Initializing {
                module.phase = Phase::Failed;
            }
        }
        let module = self.modules.modules.get_mut(&failed).unwrap();
        module.phase = Phase::Failed;
        module.references = 0;
        self.initialized.push(failed);
        // A successful reentrant load can still own a dependency on this image.
        frame.remove = self.garbage(None);
        frame.queue = self.detach_queue(&frame.remove);
        for base in &frame.remove {
            self.modules.modules.get_mut(base).unwrap().phase = Phase::Detaching;
        }
        frame.current = None;
        frame.result = 0;
        frame.error = Some(1114);
    }

    fn step_dynamic(
        &mut self,
        frame: &mut Frame,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<bool, StartupError> {
        // A later frame/output fault must not replay a completed notification.
        if let Some(current) = frame.current.take() {
            if current.require_true && state.regs[0] as u32 == 0 {
                self.rollback(frame, current.base);
            } else if current.finish {
                self.modules.modules.get_mut(&current.base).unwrap().phase = Phase::Ready;
                self.initialized.push(current.base);
            }
        }
        if let Some(notify) = frame.queue.front().copied() {
            memory.write(frame.sp, &frame_bytes(memory.mode(), &notify))?;
            state.cancel_repeat();
            state.exception = 0;
            state.ip = notify.target;
            state.regs[4] = frame.sp;
            if memory.mode() == Mode::X64 {
                state.regs[1] = notify.base;
                state.regs[2] = notify.reason;
                state.regs[8] = notify.reserved;
            }
            frame.queue.pop_front();
            frame.current = Some(notify);
            return Ok(false);
        }
        if frame.error.is_none() {
            for base in frame.ready.drain(..) {
                self.modules.modules.get_mut(&base).unwrap().phase = Phase::Ready;
                self.initialized.push(base);
            }
        }
        let error_at = context::error_address(&frame.saved, memory.mode());
        if frame.error.is_some() {
            memory.check(error_at, 4, WRITE)?;
        }
        let invalid: BTreeSet<_> = self
            .modules
            .modules
            .values()
            .filter(|m| frame.remove.contains(&m.base) || m.phase == Phase::Failed)
            .map(|m| m.base)
            .collect();
        let delay_writes = self.delays.reset_writes(&invalid, &frame.remove);
        for &(at, _) in &delay_writes {
            memory.check(at, memory.mode() as usize / 8, WRITE)?;
        }
        for base in &frame.remove {
            let module = &self.modules.modules[base];
            memory
                .check_unmap(*base, module.image.mapped_size())
                .map_err(StartupError::Invalid)?;
            if let Some(tls) = &module.tls {
                for (&id, &block) in &tls.blocks {
                    memory
                        .check_unmap(block, tls.size)
                        .map_err(StartupError::Invalid)?;
                    memory.check(
                        self.threads.array(id) + tls.slot as u64 * (memory.mode() as u64 / 8),
                        memory.mode() as usize / 8,
                        WRITE,
                    )?;
                }
            }
        }
        if matches!(frame.finish, Finish::Reject) {
            self.check_rejected_threads(memory)?;
            for (base, span) in self.mappings.regions() {
                memory
                    .check_file_view(base, 0, span as usize)
                    .map_err(StartupError::Invalid)?;
                memory
                    .check_unmap(base, span)
                    .map_err(StartupError::Invalid)?;
            }
        }
        for (at, value) in delay_writes {
            memory.write_int(at, memory.mode() as usize / 8, value)?;
        }
        self.delays.invalidate(&invalid, &frame.remove);
        for base in &frame.remove {
            let module = self.modules.modules.remove(base).unwrap();
            self.user.images.retire_instance(*base);
            self.resources.retire(*base);
            memory
                .unmap(*base, module.image.mapped_size())
                .map_err(StartupError::Invalid)?;
            if let Some(tls) = module.tls {
                for (id, block) in tls.blocks {
                    memory.write_int(
                        self.threads.array(id) + tls.slot as u64 * (memory.mode() as u64 / 8),
                        memory.mode() as usize / 8,
                        0,
                    )?;
                    memory
                        .unmap(block, tls.size)
                        .map_err(StartupError::Invalid)?;
                }
            }
        }
        if matches!(frame.finish, Finish::Reject) {
            self.clear_rejected_threads(memory);
            self.mappings
                .clear(memory)
                .expect("preflighted rejected process views");
        }
        self.initialized.retain(|base| !frame.remove.contains(base));
        self.unmapped |= !frame.remove.is_empty();
        self.modules
            .order
            .retain(|base| !frame.remove.contains(base));
        for module in self.modules.modules.values_mut() {
            module
                .dependencies
                .retain(|base| !frame.remove.contains(base));
        }
        if let Some(code) = frame.error {
            memory.write_int(error_at, 4, u64::from(code))?;
        }
        Ok(true)
    }

    pub(super) fn advance_dynamic(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
        services: &mut Services,
        files: &Files,
    ) -> Result<(), StartupError> {
        let mut frame = self.frames.pop().unwrap();
        if matches!(frame.finish, Finish::Resolve(_)) {
            return self.advance_resolution(frame, state, memory, services, files);
        }
        match self.step_dynamic(&mut frame, state, memory) {
            Ok(true) => {
                let (work, retired) = (state.work, state.retired);
                *state = *frame.saved;
                state.work = work;
                state.retired = retired;
                match frame.finish {
                    Finish::Api(call) => {
                        call.complete(state, frame.result);
                        Ok(())
                    }
                    Finish::Reject => {
                        self.process.clear();
                        self.open_files = Default::default();
                        self.file_searches.clear();
                        self.standard.close_all();
                        self.pipes.clear();
                        self.input.close();
                        Err(StartupError::Rejected)
                    }
                    Finish::Resume => {
                        self.resolution_loaded(frame.error.map_or(Ok(frame.result), Err));
                        state.ip = gateways::layout(memory.mode()).callback;
                        Ok(())
                    }
                    Finish::Thread { exit } => {
                        self.threads.notified(exit);
                        Ok(())
                    }
                    Finish::ExitThread(code) => {
                        self.exit_thread(code, state, memory.mode());
                        Ok(())
                    }
                    Finish::ExitProcess => {
                        let code = self
                            .exiting
                            .as_ref()
                            .expect("process exit continuation")
                            .code;
                        self.finish_process_exit(code, state, memory.mode());
                        Ok(())
                    }
                    Finish::Resolve(_) => unreachable!(),
                }
            }
            result => {
                self.frames.push(frame);
                result.map(|_| ())
            }
        }
    }
}

impl Frame {
    pub(super) fn resolving(saved: Box<State>, task: super::symbols::Resolve, root: u64) -> Self {
        Self {
            saved,
            finish: Finish::Resolve(task),
            created: BTreeSet::from([root]),
            queue: VecDeque::new(),
            current: None,
            ready: Vec::new(),
            edges: Vec::new(),
            remove: BTreeSet::new(),
            result: 0,
            error: None,
            sp: 0,
        }
    }
}
impl Loaded {
    fn finish_free(&mut self, finish: Finish, state: &mut State, mode: Mode) {
        match finish {
            Finish::Api(call) => call.complete(state, 1),
            Finish::ExitThread(code) => self.exit_thread(code, state, mode),
            _ => unreachable!(),
        }
    }

    fn finish_load(&mut self, finish: Finish, base: u64, state: &mut State, mode: Mode) {
        match finish {
            Finish::Api(call) => call.complete(state, base),
            Finish::Resume => {
                self.resolution_loading(base);
                self.resolution_loaded(Ok(base));
                state.ip = gateways::layout(mode).callback;
            }
            _ => unreachable!(),
        }
    }
}

impl Frame {
    pub(super) fn thread(
        state: &State,
        memory: &Memory,
        queue: VecDeque<Initializer>,
        retained: Vec<u64>,
        exit: Option<u32>,
    ) -> Result<Self, Fault> {
        let sp = if queue.is_empty() {
            state.regs[4]
        } else {
            frame_sp(state, memory.mode())?
        };
        if let Some(notify) = queue.front() {
            memory.check(sp, frame_bytes(memory.mode(), notify).len(), WRITE)?;
        }
        Ok(Self {
            saved: Box::new(state.clone()),
            finish: Finish::Thread { exit },
            queue,
            current: None,
            created: retained.into_iter().collect(),
            ready: Vec::new(),
            edges: Vec::new(),
            remove: BTreeSet::new(),
            result: 0,
            error: None,
            sp,
        })
    }
}
