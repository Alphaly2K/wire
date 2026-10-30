//! Enumeration resumes ordinary guest calls with transient stack-owned strings.
use super::{
    callbacks::{CallFrame, Callback},
    unwind, *,
};
use crate::{
    memory::FaultKind,
    win32::{call::Call, catalog::NlsApi as N, context, locale, ServiceError},
};

#[derive(Clone)]
struct Item {
    bytes: Vec<u8>,
    flags: u32,
}
#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    call: Call,
    remaining: VecDeque<Item>,
    extended: bool,
    return_sp: u64,
    continued: Option<bool>,
}
impl Frame {
    pub(super) fn unwind_bridge(&self) -> unwind::Bridge {
        let mut caller = self.saved.clone();
        self.call.clone().complete(&mut caller, 1);
        unwind::Bridge {
            sp: self.return_sp,
            caller: Some((caller, true)),
        }
    }
    fn entry(&self, memory: &Memory, item: &Item) -> Result<(u64, CallFrame), Fault> {
        let mode = memory.mode();
        let top = self.saved.regs[4];
        let buffer = if mode == Mode::X86 {
            mode.address(top.wrapping_sub(item.bytes.len() as u64))
        } else {
            top.checked_sub(item.bytes.len() as u64).ok_or(Fault {
                address: top,
                length: item.bytes.len(),
                access: WRITE,
                kind: FaultKind::Address,
            })?
        } & !15;
        let arguments = if self.extended {
            vec![buffer, item.flags.into(), self.call.arguments[2]]
        } else {
            vec![buffer]
        };
        let entry = CallFrame::new(mode, buffer, &arguments)?;
        memory.check(buffer, item.bytes.len(), WRITE)?;
        memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        Ok((buffer, entry))
    }
    fn enter(&mut self, state: &mut State, memory: &mut Memory) -> Result<(), Fault> {
        let item = self.remaining.front().expect("pending locale");
        let (buffer, entry) = self.entry(memory, item)?;
        self.return_sp = entry.sp
            + if memory.mode() == Mode::X86 {
                if self.extended {
                    16
                } else {
                    8
                }
            } else {
                8
            };
        memory.write(buffer, &item.bytes)?;
        entry.enter(self.call.arguments[0], state, memory)?;
        self.remaining.pop_front();
        self.continued = None;
        Ok(())
    }
}

impl Loaded {
    pub(crate) fn enumerate_locales(
        &mut self,
        id: N,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), ServiceError> {
        let extended = id == N::EnumSystemLocalesEx;
        let a = &call.arguments;
        let flags = a[1] as u32;
        let error = if a[0] == 0 || extended && a[3] != 0 {
            Some(87)
        } else if extended && flags & !0x37 != 0 || !extended && (flags & !7 != 0 || flags & 3 == 3)
        {
            Some(1004)
        } else {
            None
        };
        if let Some(code) = error {
            memory.write_int(context::error_address(state, memory.mode()), 4, code)?;
            call.complete(state, 0);
            return Ok(());
        }
        let mut remaining = VecDeque::new();
        for (name, kind) in locale::enumeration(if extended { flags } else { 0 }) {
            let text = if extended {
                name.to_owned()
            } else {
                if kind & 0x20 == 0 || name.is_empty() || flags == 4 {
                    continue;
                }
                format!("{:08x}", locale::id(name).expect("known locale"))
            };
            let bytes = if id == N::EnumSystemLocalesA {
                text.bytes().chain(Some(0)).collect()
            } else {
                text.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect()
            };
            remaining.push_back(Item { bytes, flags: kind });
        }
        if remaining.is_empty() {
            call.complete(state, 1);
            return Ok(());
        }
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "locale callback nesting capacity exceeded",
            ));
        }
        let mut frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            remaining,
            extended,
            return_sp: 0,
            continued: None,
        };
        frame.enter(state, memory)?;
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Locale(Box::new(frame)));
        Ok(())
    }
    pub(super) fn resume_locales(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::Locale(frame)) = self.threads.current_mut().callbacks.last_mut() else {
            return Err(unwind::invalid("locale return has no continuation"));
        };
        let continued = *frame.continued.get_or_insert(state.regs[0] as u32 != 0);
        let mut frame = frame.clone();
        let mut restored = *frame.saved.clone();
        restored.work = state.work;
        restored.retired = state.retired;
        if continued && !frame.remaining.is_empty() {
            frame.enter(&mut restored, memory)?;
            *self.threads.current_mut().callbacks.last_mut().unwrap() = Callback::Locale(frame);
        } else {
            self.threads.current_mut().callbacks.pop();
            frame.call.complete(&mut restored, 1);
        }
        *state = restored;
        memory.clear_fault();
        Ok(())
    }
}
