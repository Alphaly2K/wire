use super::*;
use crate::{jit::Exit, machine::WaitReason, win32::call::Call};
use std::collections::BTreeMap;

const QUANTUM: u64 = 10_000;
pub(crate) const ACCESS: u32 = 0x0010_0c7a;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WaitTarget {
    Process(u32),
    Primary(u32),
    Thread(u32),
    Object(u64),
}
#[derive(Clone, Copy)]
pub(crate) struct MessageWait {
    pub mask: u32,
    pub input_available: bool,
}
#[derive(Clone)]
pub(crate) enum PendingKind {
    FileInput,
    Critical,
    Inline,
    Condition(crate::win32::condition::Wait<u64>),
    Address(crate::win32::address_wait::AddressWait<u64>),
    Message,
    Sleep {
        alertable: bool,
        deadline: Option<u64>,
    },
    Wait {
        alertable: bool,
        targets: Vec<WaitTarget>,
        all: bool,
        deadline: Option<u64>,
        messages: Option<MessageWait>,
    },
}
#[derive(Clone)]
pub(crate) struct Pending {
    pub call: Call,
    pub kind: PendingKind,
    pub pc: u64,
    pub result: Option<u64>,
    scope: (usize, usize),
}
#[derive(Clone, Copy)]
enum Block {
    Loader,
    Retry(u64),
}
struct Resources {
    stack: u64,
    size: u64,
    slot: u32,
}
pub(crate) struct Thread {
    saved: State,
    teb: u64,
    array: u64,
    resources: Option<Resources>,
    pub suspended: u32,
    started: bool,
    pub exit: Option<u32>,
    exiting: Option<u32>,
    detached: bool,
    pub(super) fls_cleaned: bool,
    block: Option<Block>,
    pending: Vec<Pending>,
    pub callbacks: Vec<super::callbacks::Callback>,
    pub(crate) apcs: crate::win32::apcs::Queue,
    pub delivery: Option<super::exceptions::Delivery>,
    pub unhandled: Option<u32>,
    pub priority: i32,
    pub error_mode: u32,
    pub description: Vec<u8>,
    pub(crate) apartment: crate::win32::apartment::Apartment,
    pub(crate) impersonation: Option<u32>,
}
struct Handle {
    id: u32,
    access: u32,
    flags: u32,
}
pub(crate) struct Threads {
    pub active: u32,
    first: u32,
    entries: BTreeMap<u32, Thread>,
    handles: BTreeMap<u64, Handle>,
    next_id: u32,
    next_handle: u64,
    default_stack: u64,
    epoch: u64,
    pub(crate) next_condition: u64,
    pub(crate) next_address: u64,
    turn_started: u64,
    pub yield_requested: bool,
    mode: Mode,
}
impl Threads {
    pub(crate) fn selects_handles(&self, select: impl Fn(u64, u32) -> bool) -> bool {
        self.handles.iter().any(|(&h, v)| select(h, v.flags))
    }
    pub(super) fn new(mode: Mode, teb: u64, array: u64, default_stack: u64, first: u32) -> Self {
        Self {
            active: first,
            first,
            entries: BTreeMap::from([(
                first,
                Thread {
                    saved: State::new(mode, 0, 0),
                    teb,
                    array,
                    resources: None,
                    suspended: 0,
                    started: true,
                    exit: None,
                    exiting: None,
                    detached: false,
                    fls_cleaned: false,
                    block: None,
                    pending: Vec::new(),
                    callbacks: Vec::new(),
                    apcs: Default::default(),
                    delivery: None,
                    unhandled: None,
                    priority: 0,
                    error_mode: 0,
                    description: Vec::new(),
                    apartment: Default::default(),
                    impersonation: None,
                },
            )]),
            handles: BTreeMap::new(),
            next_id: first + 1,
            next_handle: crate::win32::standard_handles::ALIAS_END,
            default_stack,
            epoch: 0,
            next_condition: 0,
            next_address: 0,
            turn_started: 0,
            yield_requested: false,
            mode,
        }
    }
    pub(crate) fn live(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.entries
            .iter()
            .filter(|(_, t)| t.exit.is_none())
            .map(|(&id, t)| (id, t.array))
    }
    pub(super) fn array(&self, id: u32) -> u64 {
        self.entries[&id].array
    }
    pub(crate) fn current(&self) -> &Thread {
        &self.entries[&self.active]
    }
    pub(crate) fn snapshot(&self) -> impl Iterator<Item = (u32, i32)> + '_ {
        self.entries
            .iter()
            .filter(|(_, t)| t.exit.is_none())
            .map(|(&id, t)| (id, t.priority))
    }
    pub(crate) fn teb(&self) -> u64 {
        self.current().teb
    }
    pub(crate) fn tebs(&self) -> impl Iterator<Item = u64> + '_ {
        self.entries
            .values()
            .filter(|t| t.exit.is_none())
            .map(|t| t.teb)
    }
    pub(crate) fn current_mut(&mut self) -> &mut Thread {
        self.entries.get_mut(&self.active).unwrap()
    }
    pub(crate) fn primary(&mut self) -> Option<&mut Thread> {
        self.entries.get_mut(&self.first)
    }
    pub(crate) fn get(&self, id: u32) -> Option<&Thread> {
        self.entries.get(&id)
    }
    pub(crate) fn get_mut(&mut self, id: u32) -> &mut Thread {
        self.entries.get_mut(&id).unwrap()
    }
    pub(crate) fn changed(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub(crate) fn has_condition(&self, condition: u64) -> bool {
        self.entries
            .values()
            .flat_map(|t| &t.pending)
            .any(|p| matches!(&p.kind, PendingKind::Condition(w) if w.condition == condition))
    }
    pub(crate) fn condition_clock(&self, condition: u64, now: u64) -> bool {
        self.entries.values().flat_map(|t| &t.pending).any(|p| matches!(&p.kind, PendingKind::Condition(w) if w.wakeable(condition, now) && w.deadline.is_some()))
    }
    pub(crate) fn wake_condition(&mut self, condition: u64, now: u64, all: bool) {
        let first = self
            .entries
            .values()
            .flat_map(|t| &t.pending)
            .filter_map(|p| match &p.kind {
                PendingKind::Condition(w) if w.wakeable(condition, now) => Some(w.ticket),
                _ => None,
            })
            .min();
        for p in self.entries.values_mut().flat_map(|t| &mut t.pending) {
            if let PendingKind::Condition(w) = &mut p.kind {
                if w.wakeable(condition, now) && (all || Some(w.ticket) == first) {
                    w.notified = true;
                }
            }
        }
        if first.is_some() {
            self.changed();
        }
    }
    pub(crate) fn address_clock(&self, address: u64, now: u64) -> bool {
        self.entries.values().flat_map(|t| &t.pending).any(|p| matches!(&p.kind, PendingKind::Address(w) if w.wakeable(address, now) && w.deadline.is_some()))
    }
    pub(crate) fn wake_address(&mut self, address: u64, now: u64, all: bool) {
        let first = crate::win32::address_wait::first(
            self.entries
                .values()
                .flat_map(|t| &t.pending)
                .filter_map(|p| match &p.kind {
                    PendingKind::Address(w) => Some((w.ticket, w)),
                    _ => None,
                }),
            address,
            now,
        );
        for p in self.entries.values_mut().flat_map(|t| &mut t.pending) {
            if let PendingKind::Address(w) = &mut p.kind {
                if w.wakeable(address, now) && (all || Some(w.ticket) == first) {
                    w.notified = true;
                }
            }
        }
        if first.is_some() {
            self.changed();
        }
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn edited(&mut self) {
        let t = self.current_mut();
        t.block = None;
        t.pending.clear();
        t.unhandled = None;
        t.delivery = None;
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn completed(&mut self) {
        self.current_mut().block = None;
        self.changed();
    }
    pub(crate) fn target(&self, handle: u64, access: u32) -> Result<u32, u32> {
        if handle == self.mode.address(u64::MAX - 1) {
            return Ok(self.active);
        }
        let h = self.handles.get(&handle).ok_or(6u32)?;
        if h.access & access == 0 && access != 0 {
            return Err(5);
        }
        Ok(h.id)
    }
    fn capacity(&self) -> Result<(), u32> {
        if self.handles.len() >= 1024 || self.next_handle >= 0x8000_0000 {
            Err(8)
        } else {
            Ok(())
        }
    }
    fn alias(&mut self, id: u32, access: u32, inherit: bool) -> Result<u64, u32> {
        self.capacity()?;
        let handle = self.next_handle;
        self.handles.insert(
            handle,
            Handle {
                id,
                access,
                flags: u32::from(inherit),
            },
        );
        self.next_handle += 1;
        Ok(handle)
    }
    pub(crate) fn open(&mut self, id: u32, access: u32, inherit: bool) -> Result<u64, u32> {
        if !self.entries.contains_key(&id) {
            return Err(87);
        }
        let access = if access == 0x1fffff { ACCESS } else { access };
        if access & !ACCESS != 0 {
            return Err(50);
        }
        self.alias(id, access, inherit)
    }
    pub(crate) fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|h| h.flags)
    }
    pub(crate) fn set_flags(&mut self, handle: u64, mask: u32, value: u32) -> Result<(), u32> {
        let h = self.handles.get_mut(&handle).ok_or(6u32)?;
        if mask & !3 != 0 {
            return Err(87);
        }
        h.flags = (h.flags & !mask) | (value & mask);
        Ok(())
    }
    pub(crate) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        self.collect();
        Ok(())
    }
    pub(crate) fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
    ) -> Result<u64, u32> {
        let id = self.target(handle, 0)?;
        let access = self.handles.get(&handle).map_or(ACCESS, |h| h.access);
        let desired = desired.unwrap_or(access);
        if desired & !access != 0 {
            return Err(5);
        }
        self.alias(id, desired, inherit)
    }
    fn collect(&mut self) {
        let active = self.active;
        let waited: BTreeSet<_> = self
            .entries
            .values()
            .flat_map(|t| t.pending.iter())
            .flat_map(|p| match &p.kind {
                PendingKind::Wait { targets, .. } => targets.as_slice(),
                _ => &[],
            })
            .filter_map(|target| {
                if let WaitTarget::Thread(id) = target {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        self.entries.retain(|&id, t| {
            id == active
                || t.exit.is_none()
                || waited.contains(&id)
                || self.handles.values().any(|h| h.id == id)
        });
    }
    pub(crate) fn request_exit(&mut self, code: u32) {
        self.current_mut().exiting = Some(code);
    }
    pub(super) fn stop_process(&mut self, code: u32, all: bool) {
        for (&id, t) in &mut self.entries {
            if all || id != self.active {
                t.exit = Some(code);
                t.impersonation = None;
                t.apartment = Default::default();
                t.exiting = None;
                t.block = None;
                t.pending.clear();
                t.callbacks.clear();
                t.apcs.clear();
                t.delivery = None;
                t.unhandled = None;
            }
        }
        if all {
            self.handles.clear();
        }
        self.yield_requested = false;
        self.changed();
    }
    pub(super) fn notified(&mut self, exit: Option<u32>) {
        let t = self.current_mut();
        if exit.is_some() {
            t.detached = true;
        } else {
            t.started = true;
        }
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn quantum(&self, work: u64) -> u64 {
        if self.live().take(2).count() < 2 {
            return u64::MAX;
        }
        QUANTUM
            .saturating_sub(work.wrapping_sub(self.turn_started))
            .max(1)
    }
    fn runnable(&self, id: u32, loader: Option<u32>) -> bool {
        let t = &self.entries[&id];
        t.exit.is_none()
            && t.suspended == 0
            && (t.started || loader.is_none() || loader == Some(id))
            && match t.block {
                None => true,
                Some(Block::Loader) => loader.is_none() || loader == Some(id),
                Some(Block::Retry(epoch)) => epoch != self.epoch,
            }
    }
}

impl Loaded {
    pub(crate) fn thread_context(
        &mut self,
        set: bool,
        call: &Call,
        state: &State,
        memory: &mut Memory,
    ) -> Result<u64, LoadError> {
        let mode = memory.mode();
        let id = self
            .threads
            .target(call.arguments[0], if set { 0x10 } else { 8 })
            .map_err(LoadError::Windows)?;
        let target = self.threads.entries.get(&id).ok_or(LoadError::Windows(6))?;
        if target.exit.is_some() || target.exiting.is_some() {
            return Err(LoadError::Windows(6));
        }
        if set && id == self.threads.active || id != self.threads.active && target.suspended == 0 {
            return Err(LoadError::Windows(170));
        }
        if mode == Mode::X64 && call.arguments[1] & 15 != 0 {
            return Err(LoadError::Windows(998));
        }
        let mut data = vec![0; contexts::size(mode)];
        memory.read(call.arguments[1], &mut data)?;
        let offset = if mode == Mode::X86 { 0 } else { 48 };
        let flags = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        contexts::components(mode, flags).map_err(LoadError::Windows)?;
        if set {
            let restored = contexts::restore_thread(memory, call.arguments[1], &target.saved)
                .map_err(|error| match error {
                    StartupError::Fault(f) => LoadError::Fault(f),
                    _ => LoadError::Windows(87),
                })?;
            let changed_control =
                restored.ip != target.saved.ip || restored.regs[4] != target.saved.regs[4];
            if changed_control
                && (self.loader_owner == Some(id)
                    && (!self.entry_started || !self.frames.is_empty())
                    || target.callbacks.iter().any(|c| {
                        c.stack() <= restored.regs[4]
                            && match c {
                                super::callbacks::Callback::Exception(f) => {
                                    f.unwind_caller().is_none()
                                }
                                super::callbacks::Callback::ExceptionCall(f) => f.is_unwind(),
                                _ => false,
                            }
                    }))
            {
                return Err(LoadError::Windows(170));
            }
            let target = self.threads.entries.get_mut(&id).unwrap();
            target.saved = restored;
            target.delivery = None;
            target.unhandled = None;
            if changed_control {
                target.block = None;
                for pending in std::mem::take(&mut target.pending) {
                    self.release_pending(pending);
                }
                let stack = self.threads.entries[&id].saved.regs[4];
                self.discard_thread_callbacks(id, stack);
            }
            self.threads.changed();
        } else {
            let mut current;
            let saved = if id == self.threads.active {
                current = state.clone();
                call.clone().complete(&mut current, state.regs[0]);
                current.regs[0] = state.regs[0];
                &current
            } else {
                &target.saved
            };
            contexts::select(saved, mode, flags, &mut data);
            memory.write(call.arguments[1], &data)?;
        }
        Ok(1)
    }

    fn pending_scope(&self) -> (usize, usize) {
        // A handler's wait must not replace the interrupted caller's saved API.
        (
            self.callback_depth(),
            self.threads.current().callbacks.len(),
        )
    }
    pub(crate) fn pending(&self) -> Option<&Pending> {
        let scope = self.pending_scope();
        self.threads
            .current()
            .pending
            .iter()
            .find(|p| p.scope == scope)
    }
    pub(crate) fn set_pending(&mut self, call: Call, kind: PendingKind, pc: u64) {
        self.clear_pending();
        let scope = self.pending_scope();
        self.threads.current_mut().pending.push(Pending {
            call,
            kind,
            pc,
            scope,
            result: None,
        });
    }
    fn release_pending(&mut self, pending: Pending) {
        if let PendingKind::Wait { targets, .. } = pending.kind {
            for target in targets {
                if let WaitTarget::Object(id) = target {
                    self.objects.release_wait(id);
                }
            }
        }
    }
    pub(crate) fn clear_pending(&mut self) {
        let scope = self.pending_scope();
        if let Some(index) = self
            .threads
            .current()
            .pending
            .iter()
            .position(|p| p.scope == scope)
        {
            let pending = self.threads.current_mut().pending.remove(index);
            self.release_pending(pending);
        }
    }
    pub(crate) fn clear_all_pending(&mut self) {
        for pending in std::mem::take(&mut self.threads.current_mut().pending) {
            self.release_pending(pending);
        }
    }
    pub(crate) fn reconcile_pending(&mut self, state: &State) {
        let scope = self.pending_scope();
        while let Some(index) = self.threads.current().pending.iter().position(|p| {
            p.scope.0 > scope.0 || p.scope.1 > scope.1 || p.scope == scope && p.pc != state.ip
        }) {
            let pending = self.threads.current_mut().pending.remove(index);
            self.release_pending(pending);
        }
    }
    pub(crate) fn loader_available(&self) -> bool {
        self.loader_owner.is_none() || self.loader_owner == Some(self.threads.active)
    }
    fn pulse_candidates(&self, object: u64) -> Vec<(u32, usize)> {
        self.threads
            .entries
            .iter()
            .filter(|(id, t)| {
                **id != self.threads.active && t.exit.is_none() && t.exiting.is_none()
            })
            .flat_map(|(&id, t)| {
                let depth = if self.loader_owner == Some(id) {
                    self.frames.len()
                } else {
                    0
                };
                t.pending.iter().enumerate().filter_map(move |(index, p)| {
                    if p.result.is_some()
                        || p.scope != (depth, t.callbacks.len())
                        || p.pc != t.saved.ip
                    {
                        return None;
                    }
                    match &p.kind {
                        PendingKind::Wait {
                            targets, alertable, ..
                        } if targets.contains(&WaitTarget::Object(object))
                            && (!*alertable || t.apcs.is_empty()) =>
                        {
                            Some((id, index))
                        }
                        _ => None,
                    }
                })
            })
            .collect()
    }
    pub(crate) fn pulse_clock(&self, handle: u64) -> bool {
        let Ok((object, _)) = self.objects.pulse_identity(handle) else {
            return false;
        };
        self.pulse_candidates(object).iter().any(|&(id, index)| {
            matches!(&self.threads.entries[&id].pending[index].kind, PendingKind::Wait { targets, deadline, messages, .. }
                if deadline.is_some() || messages.is_some_and(|m| self.user.message_clock(id, m.mask))
                    || targets.iter().any(|t| matches!(t, WaitTarget::Object(id) if self.objects.needs_time(*id))))
        })
    }
    pub(crate) fn pulse_event(&mut self, handle: u64, sample: u64) -> Result<u64, u32> {
        let (object, manual) = self.objects.pulse_identity(handle)?;
        let candidates = self.pulse_candidates(object);
        self.objects.clock.0 = self.objects.clock.0.max(sample);
        let now = self.objects.clock.0;
        self.objects.change(handle, true)?;
        for (thread, index) in candidates {
            let pending = self.threads.entries[&thread].pending[index].clone();
            let PendingKind::Wait {
                targets,
                all,
                deadline,
                messages,
                ..
            } = pending.kind
            else {
                unreachable!()
            };
            if deadline.is_some_and(|d| now >= d) {
                continue;
            }
            if let Ok(Some(value)) = crate::win32::threads::wait_value(
                self, thread, &targets, all, deadline, messages, now,
            ) {
                self.threads.get_mut(thread).pending[index].result = Some(value);
                self.threads.changed();
            }
            if !manual && !self.objects.peek_id(object, thread)?.ready {
                break;
            }
        }
        self.objects.change(handle, false)
    }
    pub(crate) fn refresh_loader(&mut self) {
        if self.process.exit.is_none() {
            let mut images = self.process.record.images.borrow_mut();
            if images.len() != self.modules.modules.len()
                || images
                    .iter()
                    .zip(self.modules.modules.values())
                    .any(|(path, m)| !path.eq_ignore_ascii_case(&m.path))
            {
                *images = self
                    .modules
                    .modules
                    .values()
                    .map(|m| m.path.to_lowercase())
                    .collect();
            }
        }
        if self.exiting.is_some() || !self.entry_started || !self.frames.is_empty() {
            self.loader_owner.get_or_insert(self.threads.active);
        } else {
            self.loader_owner = None;
        }
    }
    pub(crate) fn create_thread(
        &mut self,
        memory: &mut Memory,
        a: &[u64; 16],
    ) -> Result<u64, LoadError> {
        if a[4] & !0x10004 != 0 {
            return Err(LoadError::Windows(87));
        }
        let ptr = memory.mode() as usize / 8;
        let inherit = if a[0] == 0 {
            false
        } else {
            let size = if ptr == 4 { 12 } else { 24 };
            memory.check(a[0], size, READ)?;
            if memory.read_int(a[0], 4)? != size as u64
                || memory.read_int(a[0] + ptr as u64, ptr)? != 0
            {
                return Err(LoadError::Windows(50));
            }
            memory.read_int(a[0] + 2 * ptr as u64, 4)? != 0
        };
        if a[5] != 0 {
            memory.check(a[5], 4, WRITE)?;
        }
        self.threads.capacity().map_err(LoadError::Windows)?;
        let id = self.threads.next_id;
        if id - self.threads.first >= 65536 {
            return Err(LoadError::Windows(8));
        }
        let size = round(a[1].max(self.threads.default_stack).max(65536), 65536)
            .ok()
            .filter(|&size| size <= 16 * 1024 * 1024)
            .ok_or(LoadError::Windows(8))?;
        let addresses = |slot: u32| {
            if ptr == 4 {
                (
                    0x1000_0000 + u64::from(slot) * 0x1000000,
                    0x7f00_0000 + u64::from(slot) * 0x10000,
                )
            } else {
                (
                    0x7fe0_0000_0000 + u64::from(slot) * 0x4000000,
                    0x7ffc_0000_0000 + u64::from(slot) * 0x10000,
                )
            }
        };
        let slot = (0..64)
            .find(|slot| {
                let (stack, teb) = addresses(*slot);
                memory.available(stack, size)
                    && memory.available(teb, 2 * PAGE)
                    && !self
                        .threads
                        .entries
                        .values()
                        .any(|t| t.resources.as_ref().is_some_and(|r| r.slot == *slot))
            })
            .ok_or(LoadError::Windows(8))?;
        let (stack, teb) = addresses(slot);
        let array = teb + PAGE;
        let mut staged = Memory::new(memory.mode());
        staged
            .map(stack, size, READ | WRITE)
            .map_err(|_| LoadError::Windows(8))?;
        staged
            .map(teb, 2 * PAGE, READ | WRITE)
            .map_err(|_| LoadError::Windows(8))?;
        let fields: &[(u64, u64)] = if ptr == 4 {
            &[
                (0, 0xffffffff),
                (4, stack + size),
                (8, stack),
                (0x18, teb),
                (0x20, u64::from(self.process.id())),
                (0x24, u64::from(id)),
                (0x2c, array),
                (0x30, self.info[5]),
            ]
        } else {
            &[
                (8, stack + size),
                (16, stack),
                (0x30, teb),
                (0x40, u64::from(self.process.id())),
                (0x48, u64::from(id)),
                (0x58, array),
                (0x60, self.info[5]),
            ]
        };
        for &(offset, value) in fields {
            staged.write_int(teb + offset, ptr, value)?;
        }
        staged.write_int(
            teb + crate::win32::context::locale_offset(memory.mode()),
            4,
            0x411,
        )?;
        let mut blocks = Vec::new();
        for (&base, module) in &self.modules.modules {
            if let Some(tls) = &module.tls {
                let block = allocate_process_data(&mut staged, memory, tls.size, &tls.template)
                    .map_err(|_| LoadError::Windows(8))?;
                staged.write_int(array + tls.slot as u64 * ptr as u64, ptr, block)?;
                blocks.push((base, block));
            }
        }
        let sp = stack + size - if ptr == 4 { 16 } else { 40 };
        staged.write_int(sp, ptr, gateways::layout(memory.mode()).thread_returned)?;
        let mut saved = State::new(memory.mode(), a[2], sp);
        if ptr == 4 {
            saved.fs_base = teb;
            staged.write_int(sp + 4, 4, a[3])?;
        } else {
            saved.gs_base = teb;
            saved.regs[1] = a[3];
        }
        memory.merge(staged).map_err(|_| LoadError::Windows(8))?;
        if a[5] != 0 {
            memory.write_int(a[5], 4, u64::from(id))?;
        }
        for (base, block) in blocks {
            self.modules
                .modules
                .get_mut(&base)
                .unwrap()
                .tls
                .as_mut()
                .unwrap()
                .blocks
                .insert(id, block);
        }
        let handle = self
            .threads
            .alias(id, ACCESS, inherit)
            .expect("preflighted thread handle");
        self.threads.next_id += 1;
        self.threads.entries.insert(
            id,
            Thread {
                saved,
                teb,
                array,
                resources: Some(Resources { stack, size, slot }),
                suspended: u32::from(a[4] & 4 != 0),
                started: false,
                exit: None,
                exiting: None,
                detached: false,
                fls_cleaned: false,
                block: None,
                pending: Vec::new(),
                callbacks: Vec::new(),
                apcs: Default::default(),
                delivery: None,
                unhandled: None,
                priority: 0,
                error_mode: 0,
                description: Vec::new(),
                apartment: Default::default(),
                impersonation: None,
            },
        );
        Ok(handle)
    }

    fn notify_thread(
        &mut self,
        state: &mut State,
        memory: &Memory,
        exit: Option<u32>,
    ) -> Result<(), StartupError> {
        let mut bases = self.initialized.clone();
        if exit.is_some() {
            bases.reverse();
        }
        let mut queue = VecDeque::new();
        for base in &bases {
            if let Some(module) = self
                .modules
                .modules
                .get(base)
                .filter(|m| m.phase == Phase::Ready)
            {
                queue.extend(notifications(module, if exit.is_some() { 3 } else { 2 }, 0));
            }
        }
        let frame = modules::Frame::thread(state, memory, queue, bases, exit)?;
        self.frames.push(frame);
        self.loader_owner = Some(self.threads.active);
        state.ip = gateways::layout(memory.mode()).callback;
        Ok(())
    }
    fn reap_thread(&mut self, memory: &mut Memory, code: u32) -> Result<(), StartupError> {
        let id = self.threads.active;
        let inet_buffer = self.winsock.inet_buffers.get(&id).copied();
        if let Some(base) = inet_buffer {
            memory
                .check_unmap(base, PAGE)
                .map_err(StartupError::Invalid)?;
        }
        let mut ranges = Vec::new();
        let ptr = memory.mode() as usize / 8;
        let array = self.threads.array(id);
        let mut writes = Vec::new();
        for module in self.modules.modules.values() {
            if let Some(tls) = &module.tls {
                if let Some(&block) = tls.blocks.get(&id) {
                    ranges.push((block, tls.size));
                    writes.push(array + tls.slot as u64 * ptr as u64);
                }
            }
        }
        if let Some(r) = &self.threads.current().resources {
            ranges.extend([(r.stack, r.size), (self.threads.current().teb, 2 * PAGE)]);
        }
        for &(base, size) in &ranges {
            memory
                .check_unmap(base, size)
                .map_err(StartupError::Invalid)?;
        }
        for &at in &writes {
            memory.check(at, ptr, WRITE)?;
        }
        for at in writes {
            memory.write_int(at, ptr, 0)?;
        }
        for (base, size) in ranges {
            memory.unmap(base, size).map_err(StartupError::Invalid)?;
        }
        if let Some(base) = inet_buffer {
            self.data_pages
                .free(memory, base)
                .expect("preflighted Winsock thread buffer");
            self.winsock.inet_buffers.remove(&id);
        }
        for module in self.modules.modules.values_mut() {
            if let Some(tls) = &mut module.tls {
                tls.blocks.remove(&id);
            }
        }
        self.clear_all_pending();
        self.objects.abandon_thread(id);
        self.user.exit_thread(id);
        let t = self.threads.current_mut();
        t.exit = Some(code);
        t.impersonation = None;
        t.apartment = Default::default();
        t.pending.clear();
        t.callbacks.clear();
        t.apcs.clear();
        t.delivery = None;
        t.unhandled = None;
        t.resources = None;
        if id == self.process.first_thread() {
            self.process.record.primary_exit.set(Some(code));
        }
        self.threads.changed();
        self.unmapped = true;
        Ok(())
    }

    /// Context switches occur only after the generated resident frame is saved.
    pub(crate) fn schedule(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
        blocked: Option<WaitReason>,
        resume: bool,
    ) -> Result<Option<Exit>, StartupError> {
        // The backend has one in-flight job; its captured API owns this thread until completion.
        if self.graphics.pending() {
            return Ok(None);
        }
        if resume {
            self.threads.changed();
        }
        if self.threads.current().unhandled.is_some() {
            return Ok(None);
        }
        self.refresh_loader();
        let points = gateways::layout(memory.mode());
        if state.ip == points.thread_returned && self.threads.current().exit.is_none() {
            if self.threads.current().exiting.is_none() {
                self.threads.request_exit(state.regs[0] as u32);
            }
            if !self.loader_available() {
                self.threads.current_mut().block = Some(Block::Loader);
            } else if !self.threads.current().fls_cleaned {
                if !self.frames.is_empty() || !self.entry_started {
                    return Err(StartupError::Invalid(
                        "thread exit during a loader callback is not implemented".into(),
                    ));
                }
                self.begin_fls_exit(state, memory)?;
                return Ok(None);
            } else if !self.threads.current().detached {
                if !self.frames.is_empty() || !self.entry_started {
                    return Err(StartupError::Invalid(
                        "thread exit during a loader callback is not implemented".into(),
                    ));
                }
                self.notify_thread(state, memory, self.threads.current().exiting)?;
                return Ok(None);
            } else {
                let code = self.threads.current().exiting.unwrap();
                self.reap_thread(memory, code)?;
                state.regs[0] = u64::from(code);
            }
        }
        if let Some(reason) = blocked {
            let block = match reason {
                WaitReason::Loader => Block::Loader,
                WaitReason::Retry => Block::Retry(self.threads.epoch),
            };
            self.threads.current_mut().block = Some(block);
        }
        let active = self.threads.active;
        let elapsed = state.work.wrapping_sub(self.threads.turn_started);
        let yielded = std::mem::take(&mut self.threads.yield_requested);
        let rotate = elapsed >= QUANTUM || yielded;
        let runnable = self.threads.runnable(active, self.loader_owner);
        if runnable && !rotate {
            if !self.threads.current().started && self.loader_owner.is_none() {
                self.notify_thread(state, memory, None)?;
            }
            return Ok(None);
        }
        // Work advances timer and I/O retry opportunities once per quantum.
        if rotate && elapsed != 0 {
            self.threads.changed();
        }
        let next = self
            .threads
            .entries
            .keys()
            .copied()
            .filter(|&id| id > active)
            .chain(
                self.threads
                    .entries
                    .keys()
                    .copied()
                    .filter(|&id| id <= active),
            )
            .find(|&id| self.threads.runnable(id, self.loader_owner));
        let Some(next) = next else {
            return Ok(Some(if self.threads.live().next().is_none() {
                Exit::ThreadReturned
            } else {
                Exit::Waiting
            }));
        };
        if next != active {
            let (work, retired) = (state.work, state.retired);
            self.threads.current_mut().saved = state.clone();
            *state = self.threads.entries[&next].saved.clone();
            state.work = work;
            state.retired = retired;
            self.threads.active = next;
            self.threads.collect();
        }
        self.threads.turn_started = state.work;
        self.threads.current_mut().block = None;
        if !self.threads.current().started && self.loader_owner.is_none() {
            self.notify_thread(state, memory, None)?;
        }
        Ok(None)
    }
}

impl Loaded {
    pub(super) fn check_rejected_threads(&self, memory: &Memory) -> Result<(), StartupError> {
        self.environment
            .check_clear(memory)
            .map_err(StartupError::Invalid)?;
        self.data_pages
            .check_clear(memory)
            .map_err(StartupError::Invalid)?;
        self.heaps
            .check_clear(memory)
            .map_err(StartupError::Invalid)?;
        self.user
            .gdi
            .check_clear(memory)
            .map_err(StartupError::Invalid)?;
        for t in self.threads.entries.values() {
            if let Some(r) = &t.resources {
                memory
                    .check_unmap(r.stack, r.size)
                    .map_err(StartupError::Invalid)?;
                memory
                    .check_unmap(t.teb, 2 * PAGE)
                    .map_err(StartupError::Invalid)?;
            }
        }
        Ok(())
    }
    pub(super) fn clear_rejected_threads(&mut self, memory: &mut Memory) {
        for t in self.threads.entries.values() {
            if let Some(r) = &t.resources {
                memory
                    .unmap(r.stack, r.size)
                    .expect("preflighted rejected thread stack");
                memory
                    .unmap(t.teb, 2 * PAGE)
                    .expect("preflighted rejected thread TEB");
            }
        }
        self.threads
            .entries
            .retain(|&id, _| id == self.threads.first);
        self.threads.handles.clear();
        self.clear_all_pending();
        self.threads.current_mut().callbacks.clear();
        self.threads.current_mut().delivery = None;
        self.threads.current_mut().unhandled = None;
        self.exception_registrations = Default::default();
        self.function_tables = Default::default();
        self.locals = Default::default();
        self.srw = Default::default();
        self.objects.clear();
        self.snapshots.clear();
        self.registry.clear();
        self.tokens.clear();
        self.rng.clear();
        self.crypto.clear();
        self.etw.clear();
        self.hstrings = Default::default();
        self.bstrings32 = Default::default();
        self.bstrings64 = Default::default();
        self.safe_arrays = Default::default();
        self.security = Default::default();
        self.winsock = Default::default();
        self.periods = Default::default();
        self.data_pages.clear(memory);
        self.threads.current_mut().description.clear();
        self.threads.current_mut().apartment = Default::default();
        self.heaps.clear(memory);
        self.environment.clear(memory);
        self.user.gdi.clear_dibs(memory);
        self.user = crate::win32::user::User::new(memory.mode());
        self.graphics = Default::default();
        self.wmi = Default::default();
        self.gdiplus = Default::default();
        self.d3d9 = Default::default();
        self.audio = Default::default();
        self.crt = Default::default();
    }
    pub(crate) fn can_exit_thread(&self) -> bool {
        self.exiting.is_none()
            && self.entry_started
            && (self.frames.is_empty() || self.loader_owner != Some(self.threads.active))
    }
    pub(crate) fn exit_thread(&mut self, code: u32, state: &mut State, mode: Mode) {
        self.threads.request_exit(code);
        state.ip = gateways::layout(mode).thread_returned;
        state.cancel_repeat();
    }
    pub(crate) fn can_switch_thread(&self) -> bool {
        self.threads
            .entries
            .keys()
            .any(|&id| id != self.threads.active && self.threads.runnable(id, self.loader_owner))
    }
}
