use super::{
    callbacks::{CallFrame, Callback},
    contexts, unwind, *,
};
use crate::win32::{call::Call, context};

#[derive(Clone, Copy)]
struct DebugOutput {
    fallback: Option<u64>,
    last_error: (u64, u32),
}
#[derive(Clone)]
struct Breakpoint {
    next: u64,
    caller: Box<State>,
}

fn extent(mode: Mode) -> usize {
    if mode == Mode::X64 {
        1616
    } else {
        256 + contexts::size(mode)
    }
}

fn block(state: &State, length: usize) -> Result<u64, StartupError> {
    (state.regs[4] & !15)
        .checked_sub(length.div_ceil(16) as u64 * 16)
        .ok_or_else(|| StartupError::Invalid("exception stack address underflow".into()))
}

fn debug_string(memory: &Memory, address: u64, wide: bool) -> Result<Vec<u16>, StartupError> {
    let mut units = Vec::new();
    let width = if wide { 2 } else { 1 };
    for n in 0..65536 {
        let offset = n * width as u64;
        let at = if memory.mode() == Mode::X86 {
            memory.mode().address(address.wrapping_add(offset))
        } else {
            address.checked_add(offset).ok_or(Fault {
                address,
                length: width,
                access: READ,
                kind: crate::memory::FaultKind::Address,
            })?
        };
        let value = memory.read_int(at, width)? as u16;
        if value == 0 {
            return Ok(units);
        }
        units.push(value);
    }
    Err(StartupError::Invalid(
        "debug string capacity exceeded".into(),
    ))
}

fn encode_record(
    mode: Mode,
    code: u32,
    flags: u32,
    address: u64,
    information: &[u64],
) -> [u8; 160] {
    let mut bytes = [0; 160];
    let width = mode as usize / 8;
    bytes[..4].copy_from_slice(&code.to_le_bytes());
    bytes[4..8].copy_from_slice(&flags.to_le_bytes());
    let at = if mode == Mode::X86 { 12 } else { 16 };
    bytes[at..at + width].copy_from_slice(&address.to_le_bytes()[..width]);
    let count = if mode == Mode::X86 { 16 } else { 24 };
    bytes[count..count + 4].copy_from_slice(&(information.len() as u32).to_le_bytes());
    let at = if mode == Mode::X86 { 20 } else { 32 };
    for (n, value) in information.iter().enumerate() {
        bytes[at + n * width..at + (n + 1) * width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
    bytes
}

#[derive(Clone)]
struct Record {
    code: u32,
    flags: u32,
    address: u64,
    information: Vec<u64>,
    returned: bool,
    debug: Option<DebugOutput>,
    data: Vec<u8>,
    breakpoint: Option<Breakpoint>,
}
#[derive(Clone)]
pub(crate) struct Delivery {
    saved: Box<State>,
    record: Record,
}

#[derive(Clone)]
enum Phase {
    Vectored,
    Chain { next: u64 },
    Filter,
    Language,
    Done,
}
#[derive(Clone, Copy)]
enum HandlerKind {
    Vectored,
    Chain,
    Filter,
    Language,
    Unwind,
}
enum Action {
    Handler(u64, Vec<u64>, HandlerKind),
    Lookup(unwind::Request),
}
#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    block: u64,
    record_address: u64,
    context_address: u64,
    history: u64,
    code: u32,
    flags: u32,
    targets: VecDeque<u64>,
    filter: u64,
    phase: Phase,
    seen: BTreeSet<u64>,
    active: Option<HandlerKind>,
    return_sp: Option<u64>,
    returned: bool,
    debug: Option<DebugOutput>,
    breakpoint: Option<Breakpoint>,
    walk: Option<unwind::Walk>,
    language: Option<unwind::Handler>,
    unwind: Option<unwind::Plan>,
    planning: Option<unwind::Planner>,
    invocation: Option<unwind::Invocation>,
}
impl Frame {
    fn new(saved: State, loader_depth: usize, block: u64, record: &Record) -> Self {
        Self {
            saved: Box::new(saved),
            loader_depth,
            block,
            record_address: block,
            context_address: block + 256,
            history: 0,
            code: record.code,
            flags: record.flags,
            targets: VecDeque::new(),
            filter: 0,
            phase: Phase::Done,
            seen: BTreeSet::new(),
            active: None,
            return_sp: None,
            returned: record.returned,
            debug: record.debug,
            breakpoint: record.breakpoint.clone(),
            walk: None,
            language: None,
            unwind: None,
            planning: None,
            invocation: None,
        }
    }
    fn pointers(&self) -> u64 {
        self.block + 160
    }
    fn context(&self) -> u64 {
        self.context_address
    }
    pub(super) fn unwind_caller(&self) -> Option<(Box<State>, bool)> {
        // Leaving an active unwind needs the collided-unwind protocol.
        (self.planning.is_none() && self.unwind.is_none()).then(|| {
            self.breakpoint.as_ref().map_or_else(
                || (self.saved.clone(), self.returned),
                |breakpoint| (breakpoint.caller.clone(), true),
            )
        })
    }
    pub(super) fn unwind_bridge(&self) -> Option<unwind::Bridge> {
        self.active.map(|_| unwind::Bridge {
            sp: self.return_sp.unwrap(),
            caller: self.unwind_caller(),
        })
    }
    fn next(
        &mut self,
        memory: &Memory,
        tables: &unwind::Tables,
        modules: &Linked,
        ready: bool,
    ) -> Result<Option<Action>, StartupError> {
        if let Some(planner) = self.planning.as_mut() {
            match planner.advance(memory, tables, modules)? {
                unwind::Planning::Ready(plan) => {
                    self.unwind = Some(plan);
                    self.planning = None;
                }
                unwind::Planning::Lookup(request) => return Ok(Some(Action::Lookup(request))),
            }
        }
        if let Some(plan) = self.unwind.as_mut() {
            let Some(invocation) = plan.calls.pop_front() else {
                return Ok(None);
            };
            let h = &invocation.handler;
            let action = (
                h.target,
                vec![
                    self.record_address,
                    h.frame,
                    self.context(),
                    self.block + 1536,
                ],
                HandlerKind::Unwind,
            );
            self.language = Some(h.clone());
            self.invocation = Some(invocation);
            return Ok(Some(Action::Handler(action.0, action.1, action.2)));
        }
        loop {
            match self.phase {
                Phase::Vectored => {
                    if let Some(target) = self.targets.pop_front() {
                        return Ok(Some(Action::Handler(
                            target,
                            vec![self.pointers()],
                            HandlerKind::Vectored,
                        )));
                    }
                    if self.debug.is_some() {
                        self.phase = Phase::Done;
                        continue;
                    }
                    self.phase = if memory.mode() == Mode::X86 {
                        Phase::Chain {
                            next: memory.read_int(self.saved.fs_base, 4)?,
                        }
                    } else {
                        Phase::Language
                    };
                    if memory.mode() == Mode::X64 && !ready {
                        return Ok(None);
                    }
                }
                Phase::Chain { next } => {
                    if next == 0 || next == u64::from(u32::MAX) {
                        self.phase = Phase::Filter;
                        continue;
                    }
                    let limit = memory.read_int(self.saved.fs_base + 8, 4)?;
                    let top = memory.read_int(self.saved.fs_base + 4, 4)?;
                    if next & 3 != 0
                        || next < limit
                        || next + 8 > top
                        || self.seen.len() >= 128
                        || !self.seen.insert(next)
                    {
                        return Err(StartupError::Invalid(
                            "invalid or cyclic exception registration chain".into(),
                        ));
                    }
                    let following = memory.read_int(next, 4)?;
                    let target = memory.read_int(next + 4, 4)?;
                    self.phase = Phase::Chain { next: following };
                    return Ok(Some(Action::Handler(
                        target,
                        vec![self.record_address, next, self.context(), 0],
                        HandlerKind::Chain,
                    )));
                }
                Phase::Filter => {
                    self.phase = Phase::Done;
                    if self.filter != 0 {
                        return Ok(Some(Action::Handler(
                            self.filter,
                            vec![self.pointers()],
                            HandlerKind::Filter,
                        )));
                    }
                }
                Phase::Language => {
                    if self.walk.is_none() {
                        let mut caller = contexts::restore(memory, self.context(), &self.saved)?;
                        let mut returned = self.returned;
                        if self.breakpoint.as_ref().is_some_and(|breakpoint| {
                            caller.ip == breakpoint.next || caller.ip == breakpoint.next - 1
                        }) {
                            caller = unwind::step(memory, &caller, None, caller.ip)?.caller;
                            returned = true;
                        }
                        self.walk = Some(unwind::Walk::new(caller, returned));
                    }
                    loop {
                        match self.walk.as_mut().unwrap().next(memory, tables, modules)? {
                            unwind::WalkStep::End => break,
                            unwind::WalkStep::Lookup(request) => {
                                return Ok(Some(Action::Lookup(request)))
                            }
                            unwind::WalkStep::Frame(step) => {
                                if let Some(h) = step.handler.filter(|h| h.flags & 1 != 0) {
                                    let action = Action::Handler(
                                        h.target,
                                        vec![
                                            self.record_address,
                                            h.frame,
                                            self.context(),
                                            self.block + 1536,
                                        ],
                                        HandlerKind::Language,
                                    );
                                    self.language = Some(h);
                                    return Ok(Some(action));
                                }
                            }
                        }
                    }
                    self.phase = Phase::Filter;
                }
                Phase::Done => return Ok(None),
            }
        }
    }

    pub(super) fn resolve_lookup(
        &mut self,
        pc: u64,
        function: Option<unwind::Function>,
    ) -> Result<(), StartupError> {
        if let Some(planning) = self.planning.as_mut() {
            planning.resolve(pc, function)
        } else {
            self.walk
                .as_mut()
                .ok_or_else(|| unwind::invalid("function lookup has no stack walk"))?
                .resolve(pc, function)
        }
    }
    fn enter(
        &mut self,
        target: u64,
        args: &[u64],
        kind: HandlerKind,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let call = CallFrame::new(memory.mode(), self.block, args)?;
        memory.check(call.sp, call.bytes.len(), WRITE)?;
        let mut dispatcher = [0; 80];
        if matches!(kind, HandlerKind::Language | HandlerKind::Unwind) {
            memory.check(self.block + 1536, 80, WRITE)?;
            let h = self.language.as_ref().unwrap();
            for (at, value) in [
                (0, h.control),
                (8, h.base),
                (16, h.entry),
                (24, h.frame),
                (32, self.unwind.as_ref().map_or(0, |p| p.target.ip)),
                (40, self.context()),
                (48, h.target),
                (56, h.data),
                (64, self.history),
            ] {
                dispatcher[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
            if matches!(kind, HandlerKind::Unwind) {
                memory.check(self.context(), contexts::size(Mode::X64), WRITE)?;
                memory.check(self.record_address + 4, 4, WRITE)?;
            }
            memory.write(self.block + 1536, &dispatcher)?;
            if let Some(invocation) = self
                .invocation
                .as_ref()
                .filter(|_| matches!(kind, HandlerKind::Unwind))
            {
                memory.write(
                    self.context(),
                    &contexts::capture(&invocation.state, Mode::X64),
                )?;
                memory.write_int(
                    self.record_address + 4,
                    4,
                    u64::from(self.flags | 2 | if invocation.target { 32 } else { 0 }),
                )?;
            }
        }
        let return_sp = call.sp + memory.mode() as u64 / 8;
        call.enter(target, state, memory)?;
        self.return_sp = Some(return_sp);
        Ok(())
    }
}

impl Loaded {
    fn perform_exception_action(
        &mut self,
        mut frame: Frame,
        action: Action,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        match action {
            Action::Handler(target, args, kind) => {
                frame.enter(target, &args, kind, state, memory)?;
                frame.active = Some(kind);
                *self.threads.current_mut().callbacks.last_mut().unwrap() =
                    Callback::Exception(Box::new(frame));
            }
            Action::Lookup(request) => {
                frame.active = None;
                let index = self.threads.current().callbacks.len() - 1;
                self.begin_lookup(
                    request,
                    super::function_lookup::Finish::Exception,
                    state.regs[4].min(frame.block),
                    state,
                    memory,
                )?;
                self.threads.current_mut().callbacks[index] = Callback::Exception(Box::new(frame));
            }
        }
        Ok(())
    }
    pub(crate) fn unwind_exception(
        &mut self,
        call: &Call,
        extended: bool,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), crate::win32::ServiceError> {
        use crate::win32::ServiceError;
        let a = &call.arguments;
        if memory.mode() != Mode::X64 || a[0] == 0 {
            return Err(ServiceError::Unavailable(
                "AMD64 exit unwind is not implemented",
            ));
        }
        let result = self.begin_target_unwind(call, extended, state, memory);
        result.map_err(|e| match e {
            StartupError::Fault(f) => ServiceError::Fault(f),
            _ => ServiceError::Unavailable("invalid or unsupported AMD64 target unwind"),
        })
    }

    fn begin_target_unwind(
        &mut self,
        call: &Call,
        extended: bool,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        if self.threads.current().callbacks.len() >= 32 {
            return Err(unwind::invalid(
                "exception/callback nesting capacity exceeded",
            ));
        }
        let a = &call.arguments;
        if extended && a[4] & 15 != 0 {
            return Err(unwind::invalid("unaligned AMD64 unwind CONTEXT"));
        }
        let mut saved = state.clone();
        call.clone().complete(&mut saved, a[3]);
        let block = (state.regs[4] & !15)
            .checked_sub(1616)
            .ok_or_else(|| unwind::invalid("unwind stack address underflow"))?;
        let mut bytes = [0; 152];
        if a[2] != 0 {
            memory.read(a[2], &mut bytes)?;
        } else {
            bytes[..4].copy_from_slice(&0xc0000027u32.to_le_bytes());
            bytes[16..24].copy_from_slice(&saved.ip.to_le_bytes());
        }
        let code = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let flags = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if u32::from_le_bytes(bytes[24..28].try_into().unwrap()) > 15
            || matches!(code, 0x80000026 | 0x80000029)
        {
            return Err(unwind::invalid("unsupported AMD64 unwind exception record"));
        }
        let record = Record {
            code,
            flags,
            address: saved.ip,
            information: Vec::new(),
            returned: true,
            debug: None,
            data: Vec::new(),
            breakpoint: None,
        };
        let mut frame = Frame::new(saved.clone(), self.callback_depth(), block, &record);
        frame.record_address = if a[2] == 0 { block } else { a[2] };
        if extended {
            frame.context_address = a[4];
            frame.history = a[5];
        }
        frame.planning = Some(unwind::Planner::new(
            saved.clone(),
            true,
            [a[0], a[1], a[3]],
            self.unwind_bridges(),
        )?);
        memory.check(block, 1616, WRITE)?;
        memory.check(frame.record_address + 4, 4, WRITE)?;
        memory.check(frame.context(), contexts::size(Mode::X64), WRITE)?;
        if a[2] == 0 {
            memory.write(frame.record_address, &bytes)?;
        }
        memory.write_int(frame.record_address + 4, 4, u64::from(flags | 2))?;
        memory.write(frame.context(), &contexts::capture(&saved, Mode::X64))?;
        state.ip = gateways::layout(Mode::X64).callback;
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Exception(Box::new(frame)));
        memory.clear_fault();
        Ok(())
    }

    pub(crate) fn output_debug_string(
        &mut self,
        wide: bool,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), crate::win32::ServiceError> {
        use crate::win32::ServiceError;
        if wide && call.arguments[0] == 0 {
            call.complete(state, 0);
            return Ok(());
        }
        let result = (|| -> Result<(), StartupError> {
            let mode = memory.mode();
            let pointer = call.arguments[0];
            let units = if pointer == 0 {
                Vec::new()
            } else {
                debug_string(memory, pointer, wide)?
            };
            let data = if wide {
                let (mut bytes, _) =
                    crate::codepage::encode(&String::from_utf16_lossy(&units), 932).map_err(
                        |_| StartupError::Invalid("debug string conversion failed".into()),
                    )?;
                bytes.push(0);
                bytes
            } else if pointer == 0 {
                vec![0]
            } else {
                Vec::new()
            };
            let scratch = block(state, extent(mode) + data.len())? + extent(mode) as u64;
            let information = if wide {
                vec![
                    (units.len() + 1) as u64,
                    pointer,
                    data.len() as u64,
                    scratch,
                ]
            } else {
                vec![
                    (units.len() + 1) as u64,
                    if pointer == 0 { scratch } else { pointer },
                ]
            };
            let error = context::error_address(state, mode);
            let last_error = (error, memory.read_int(error, 4)? as u32);
            let mut saved = state.clone();
            call.complete(&mut saved, 0);
            let record = Record {
                code: if wide { 0x4001000a } else { 0x40010006 },
                flags: 0,
                address: saved.ip,
                information,
                returned: true,
                debug: Some(DebugOutput {
                    fallback: wide.then_some(scratch),
                    last_error,
                }),
                data,
                breakpoint: None,
            };
            self.begin_exception(saved, record, state, memory)
        })();
        result.map_err(|e| match e {
            StartupError::Fault(f) => ServiceError::Fault(f),
            StartupError::Invalid(_) => {
                ServiceError::Unavailable("debug string or callback capacity exceeded")
            }
            _ => unreachable!(),
        })
    }

    pub(crate) fn raise_exception(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), crate::win32::ServiceError> {
        use crate::win32::ServiceError;
        let a = call.arguments;
        let mode = memory.mode();
        let width = mode as usize / 8;
        let count = (a[2] as usize).min(15);
        let mut bytes = vec![0; count * width];
        if a[3] != 0 {
            memory.read(a[3], &mut bytes)?;
        }
        let information = if a[3] == 0 {
            Vec::new()
        } else {
            bytes
                .chunks_exact(width)
                .map(|chunk| {
                    let mut word = [0; 8];
                    word[..width].copy_from_slice(chunk);
                    u64::from_le_bytes(word)
                })
                .collect()
        };
        let mut saved = state.clone();
        call.complete(&mut saved, 0);
        let record = Record {
            code: a[0] as u32 & !0x10000000,
            flags: a[1] as u32 & 1,
            address: saved.ip,
            information,
            returned: true,
            debug: None,
            data: Vec::new(),
            breakpoint: None,
        };
        self.begin_exception(saved, record, state, memory)
            .map_err(|e| match e {
                StartupError::Fault(f) => ServiceError::Fault(f),
                StartupError::Invalid(_) => {
                    ServiceError::Unavailable("invalid exception frame or registration chain")
                }
                _ => unreachable!(),
            })
    }

    fn begin_exception(
        &mut self,
        saved: State,
        record: Record,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        if self.threads.current().callbacks.len() >= 32 {
            return Err(StartupError::Invalid(
                "exception/callback nesting capacity exceeded".into(),
            ));
        }
        let mode = memory.mode();
        let width = mode as usize / 8;
        let length = extent(mode) + record.data.len();
        let block = block(state, length)?;
        let mut frame = Frame::new(saved, self.callback_depth(), block, &record);
        frame.targets = self.exception_registrations.targets().into();
        frame.filter = self.exception_registrations.filter();
        frame.phase = Phase::Vectored;
        let first = frame.next(memory, &self.function_tables, &self.modules, false)?;
        let callback = first
            .map(|action| match action {
                Action::Handler(target, args, kind) => {
                    CallFrame::new(mode, block, &args).map(|call| (target, call, kind))
                }
                Action::Lookup(_) => unreachable!("initial exception setup defers function lookup"),
            })
            .transpose()?;
        memory.check(block, length, WRITE)?;
        if let Some((_, call, _)) = &callback {
            memory.check(call.sp, call.bytes.len(), WRITE)?;
        }
        let mut bytes = vec![0; length];
        bytes[..160].copy_from_slice(&encode_record(
            mode,
            frame.code,
            frame.flags,
            record.address,
            &record.information,
        ));
        bytes[160..160 + width].copy_from_slice(&block.to_le_bytes()[..width]);
        bytes[160 + width..160 + 2 * width]
            .copy_from_slice(&frame.context().to_le_bytes()[..width]);
        bytes[256..256 + contexts::size(mode)]
            .copy_from_slice(&contexts::capture(&frame.saved, mode));
        bytes[extent(mode)..].copy_from_slice(&record.data);
        memory.write(block, &bytes)?;
        if let Some((target, call, kind)) = callback {
            frame.return_sp = Some(call.sp + width as u64);
            call.enter(target, state, memory)?;
            frame.active = Some(kind);
        } else {
            state.ip = gateways::layout(mode).callback;
        }
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Exception(Box::new(frame)));
        memory.clear_fault();
        Ok(())
    }

    pub(crate) fn debug_break(
        &mut self,
        call: &Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), crate::win32::ServiceError> {
        use crate::win32::ServiceError;
        let mode = memory.mode();
        let mut caller = state.clone();
        call.clone().complete(&mut caller, state.regs[0]);
        let record = Record {
            code: 0x80000003,
            flags: 0,
            address: state.ip,
            information: vec![0; if mode == Mode::X86 { 3 } else { 1 }],
            returned: false,
            debug: None,
            data: Vec::new(),
            breakpoint: Some(Breakpoint {
                next: state.ip + 1,
                caller: Box::new(caller),
            }),
        };
        self.begin_exception(state.clone(), record, state, memory)
            .map_err(|error| match error {
                StartupError::Fault(fault) => ServiceError::Fault(fault),
                _ => ServiceError::Unavailable("invalid breakpoint exception frame"),
            })
    }

    pub(crate) fn deliver_pending(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let delivery = self.threads.current().delivery.as_ref().unwrap().clone();
        self.begin_exception(*delivery.saved, delivery.record, state, memory)?;
        self.threads.current_mut().delivery = None;
        Ok(())
    }

    pub(crate) fn deliver_hardware(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
        reason: crate::jit::Exit,
    ) -> Result<bool, StartupError> {
        let chain = if memory.mode() == Mode::X86 {
            let mut bytes = [0; 4];
            memory.observe(state.fs_base, &mut bytes).is_ok()
                && !matches!(u32::from_le_bytes(bytes), 0 | u32::MAX)
        } else {
            false
        };
        if self.exception_registrations.is_empty()
            && !(memory.mode() == Mode::X86 && self.exception_registrations.filter() != 0)
            && !chain
            && !(memory.mode() == Mode::X64
                && (self.exception_registrations.filter() != 0
                    || self.function_tables.any()
                    || self
                        .modules
                        .modules
                        .values()
                        .any(|m| m.image.dir(3).size != 0)))
        {
            return Ok(false);
        }
        let mut information = Vec::new();
        let code = if reason == crate::jit::Exit::Fault {
            let Some(fault) = memory.fault() else {
                return Ok(false);
            };
            information.extend([
                if fault.access & crate::memory::EXECUTE != 0 {
                    8
                } else if fault.access & WRITE != 0 {
                    1
                } else {
                    0
                },
                fault.address,
            ]);
            if fault.kind == crate::memory::FaultKind::Guard {
                0x80000001
            } else {
                0xc0000005
            }
        } else if reason == crate::jit::Exit::Exception {
            match state.exception.checked_sub(1) {
                Some(0) => 0xc0000094,
                Some(6) => 0xc000001d,
                Some(13) => {
                    information.extend([0, 0]);
                    0xc0000005
                }
                Some(16 | 19) => {
                    let status = if state.exception == 17 {
                        u32::from(state.fpu.sw & !state.fpu.cw & 0x3f)
                    } else {
                        state.mxcsr & !(state.mxcsr >> 7) & 0x3f
                    };
                    if status & 1 != 0 {
                        if state.exception == 17 && state.fpu.sw & 0x40 != 0 {
                            0xc0000092
                        } else {
                            0xc0000090
                        }
                    } else if status & 2 != 0 {
                        0xc000008d
                    } else if status & 4 != 0 {
                        0xc000008e
                    } else if status & 8 != 0 {
                        0xc0000091
                    } else if status & 16 != 0 {
                        0xc0000093
                    } else {
                        0xc000008f
                    }
                }
                _ => return Ok(false),
            }
        } else {
            return Ok(false);
        };
        self.threads.current_mut().delivery = Some(Delivery {
            saved: Box::new(state.clone()),
            record: Record {
                code,
                flags: 0,
                address: state.ip,
                information,
                returned: false,
                debug: None,
                data: Vec::new(),
                breakpoint: None,
            },
        });
        self.deliver_pending(state, memory)?;
        Ok(true)
    }

    pub(super) fn resume_exception(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        if let Some(code) = self.threads.current().unhandled {
            return Err(StartupError::Unhandled(code));
        }
        let mut frame = self
            .threads
            .current()
            .callbacks
            .last()
            .and_then(|c| c.exception())
            .unwrap()
            .clone();
        if let Some(kind) = frame.active {
            let disposition = state.regs[0] as u32;
            // Chain handlers use EXCEPTION_DISPOSITION; VEH/filter use LONG.
            let chain = matches!(kind, HandlerKind::Chain | HandlerKind::Language);
            let handled = if matches!(kind, HandlerKind::Unwind) {
                false
            } else if chain {
                disposition == 0
            } else {
                disposition == u32::MAX
            };
            if handled {
                if frame.flags & 1 != 0 {
                    return Err(StartupError::Invalid(
                        "continued a noncontinuable guest exception".into(),
                    ));
                }
                let mut continuation = contexts::restore(memory, frame.context(), &frame.saved)?;
                if frame
                    .breakpoint
                    .as_ref()
                    .is_some_and(|breakpoint| breakpoint.next == continuation.ip)
                {
                    let mode = memory.mode();
                    let width = mode as usize / 8;
                    let sp = continuation.regs[4];
                    let target = memory.read_int(sp, width)?;
                    continuation.regs[4] = if mode == Mode::X86 {
                        mode.address(sp.wrapping_add(width as u64))
                    } else {
                        sp.checked_add(width as u64)
                            .ok_or_else(|| unwind::invalid("breakpoint return stack overflow"))?
                    };
                    continuation.ip = target;
                }
                continuation.work = state.work;
                continuation.retired = state.retired;
                *state = continuation;
                self.threads.current_mut().callbacks.pop();
                memory.clear_fault();
                return Ok(());
            }
            let searching = if chain || matches!(kind, HandlerKind::Unwind) {
                disposition == 1
            } else {
                disposition == 0
            };
            if !searching {
                return Err(StartupError::Invalid(
                    "unsupported guest exception disposition".into(),
                ));
            }
        }
        if let Some(action) = frame.next(memory, &self.function_tables, &self.modules, true)? {
            self.perform_exception_action(frame, action, state, memory)?;
            return Ok(());
        }
        if let Some(mut debug) = frame.debug {
            if let Some(pointer) = debug.fallback.take() {
                let count = debug_string(memory, pointer, false)?.len() as u64 + 1;
                debug.last_error.1 = memory.read_int(debug.last_error.0, 4)? as u32;
                let record = encode_record(
                    memory.mode(),
                    0x40010006,
                    0,
                    frame.saved.ip,
                    &[count, pointer],
                );
                let context = contexts::capture(&frame.saved, memory.mode());
                memory.check(frame.record_address, record.len(), WRITE)?;
                memory.check(frame.context(), context.len(), WRITE)?;
                memory.write(frame.record_address, &record)?;
                memory.write(frame.context(), &context)?;
                frame.code = 0x40010006;
                frame.debug = Some(debug);
                frame.targets = self.exception_registrations.targets().into();
                frame.phase = Phase::Vectored;
                frame.active = None;
                self.threads.current_mut().callbacks.pop();
                self.threads
                    .current_mut()
                    .callbacks
                    .push(Callback::Exception(Box::new(frame)));
                state.ip = gateways::layout(memory.mode()).callback;
                return Ok(());
            }
            memory.write_int(debug.last_error.0, 4, u64::from(debug.last_error.1))?;
            let (work, retired) = (state.work, state.retired);
            *state = *frame.saved;
            state.work = work;
            state.retired = retired;
            self.threads.current_mut().callbacks.pop();
            memory.clear_fault();
            return Ok(());
        }
        if let Some(plan) = frame.unwind {
            memory.write(
                frame.context_address,
                &contexts::capture(&plan.target, Mode::X64),
            )?;
            let (work, retired) = (state.work, state.retired);
            *state = *plan.target;
            state.work = work;
            state.retired = retired;
            self.threads.current_mut().callbacks.pop();
            self.discard_callbacks(state.regs[4]);
            memory.clear_fault();
            return Ok(());
        }
        let (work, retired) = (state.work, state.retired);
        *state = *frame.saved;
        state.work = work;
        state.retired = retired;
        self.threads.current_mut().callbacks.pop();
        self.threads.current_mut().unhandled = Some(frame.code);
        Err(StartupError::Unhandled(frame.code))
    }

    pub(crate) fn exception_registration(
        &mut self,
        id: crate::win32::catalog::ExceptionApi,
        call: &Call,
        state: &State,
        memory: &mut Memory,
    ) -> Result<u64, crate::win32::ServiceError> {
        use crate::win32::catalog::ExceptionApi as E;
        Ok(match id {
            E::AddTable | E::DeleteTable | E::InstallCallback => self
                .function_tables
                .call(memory, id, &call.arguments)
                .map_err(|e| match e {
                    StartupError::Fault(f) => crate::win32::ServiceError::Fault(f),
                    _ => crate::win32::ServiceError::Unavailable(
                        "invalid or unsupported AMD64 function metadata",
                    ),
                })?,
            E::AddVectored => match self
                .exception_registrations
                .add(call.arguments[0] != 0, call.arguments[1])
            {
                Ok(token) => token,
                Err(code) => {
                    memory.write_int(
                        context::error_address(state, memory.mode()),
                        4,
                        u64::from(code),
                    )?;
                    0
                }
            },
            E::RemoveVectored => self.exception_registrations.remove(call.arguments[0]),
            E::SetFilter => self.exception_registrations.set_filter(call.arguments[0]),
            _ => unreachable!(),
        })
    }
}
