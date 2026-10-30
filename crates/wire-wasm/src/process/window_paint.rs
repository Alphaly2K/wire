//! Paint requests keep their DC and completed erase result across output faults.
use super::*;
use crate::win32::catalog::PaintingApi as P;
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Begin,
    Query,
    Default,
}
#[derive(Clone)]
pub(super) struct Paint {
    pub dc: u64,
    window: u64,
    output: u64,
    area: [i32; 4],
    kind: Kind,
    version: u64,
    exists: bool,
}
impl Loaded {
    pub(crate) fn painting_call(
        &mut self,
        id: P,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        if matches!(id, P::Begin | P::UpdateRect) {
            return self.begin_paint(
                call,
                state,
                memory,
                services,
                if id == P::Begin {
                    Kind::Begin
                } else {
                    Kind::Query
                },
            );
        }
        let value =
            self.user
                .painting_value(id, &call.arguments, state, memory, self.threads.active)?;
        call.complete(state, value);
        Ok(())
    }
    pub(super) fn default_paint(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        self.begin_paint(call, state, memory, services, Kind::Default)
    }
    fn begin_paint(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
        kind: Kind,
    ) -> Result<(), ServiceError> {
        let a = call.arguments;
        let tid = self.threads.active;
        let mode = memory.mode();
        let Some(w) = self.user.windows.get(&a[0]) else {
            let v = user::error(memory, state, 1400)?;
            call.complete(state, v);
            return Ok(());
        };
        if w.tid != u64::from(tid) {
            let v = user::error(memory, state, 50)?;
            call.complete(state, v);
            return Ok(());
        }
        let window = a[0];
        let output = if matches!(kind, Kind::Default) {
            0
        } else {
            a[1]
        };
        let length = if mode == Mode::X86 { 64 } else { 72 };
        if matches!(kind, Kind::Begin) {
            memory.check(output, length, WRITE)?;
        } else if output != 0 {
            memory.check(output, 16, WRITE)?;
        }
        let region = self.user.paints.get(&window);
        let area = region.and_then(|p| p.area).unwrap_or([0; 4]);
        let exists = region.is_some_and(|p| p.area.is_some());
        let version = region.map_or(0, |p| p.version);
        let erase = exists
            && region.is_some_and(|p| p.erase)
            && (!matches!(kind, Kind::Query) || a[2] != 0);
        if matches!(kind, Kind::Query) && !erase {
            if output != 0 {
                user::write_rect(memory, output, area)?;
            }
            call.complete(state, u64::from(exists));
            return Ok(());
        }
        if matches!(kind, Kind::Default) && !exists {
            call.complete(state, 0);
            return Ok(());
        }
        let block = state.regs[4] & !15;
        let entry = CallFrame::new(mode, block, &[0; 4])?;
        if erase {
            if self.threads.current().callbacks.len() >= 32 {
                return Err(ServiceError::Unavailable(
                    "USER callback nesting capacity exceeded",
                ));
            }
            user::procedure(memory, services, w.procedure)?;
            memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        }
        let (width, height) = (
            (w.client[2] - w.client[0]) as u32,
            (w.client[3] - w.client[1]) as u32,
        );
        let dc = match self
            .user
            .gdi
            .acquire(window, tid, width, height, Some(area))
        {
            Ok(h) => h,
            Err(e) => {
                let v = user::error(memory, state, e)?;
                call.complete(state, v);
                return Ok(());
            }
        };
        let paint = Paint {
            dc,
            window,
            output,
            area,
            kind,
            version,
            exists,
        };
        if !matches!(kind, Kind::Query) {
            self.user.validate(window, None);
        }
        if erase {
            let entry = CallFrame::new(mode, block, &[window, 0x14, dc, 0])?;
            let frame = Frame {
                saved: Box::new(state.clone()),
                loader_depth: self.callback_depth(),
                call,
                operation: Operation::Paint(paint),
                block,
                return_sp: entry.sp + if mode == Mode::X86 { 20 } else { 8 },
            };
            entry.enter(self.user.windows[&window].procedure, state, memory)?;
            self.threads
                .current_mut()
                .callbacks
                .push(Callback::Window(Box::new(frame)));
        } else {
            let value = self
                .finish_paint(&paint, true, state, memory)
                .map_err(|e| match e {
                    StartupError::Fault(f) => ServiceError::Fault(f),
                    _ => ServiceError::Unavailable("paint completion failed"),
                })?;
            call.complete(state, value);
        }
        Ok(())
    }
    pub(super) fn finish_paint(
        &mut self,
        p: &Paint,
        erased: bool,
        state: &State,
        memory: &mut Memory,
    ) -> Result<u64, StartupError> {
        if !self.user.windows.contains_key(&p.window) {
            self.user.gdi.cancel_paint(p.dc);
            memory.write_int(
                crate::win32::context::error_address(state, memory.mode()),
                4,
                1400,
            )?;
            return Ok(0);
        }
        match p.kind {
            Kind::Begin => {
                let mode = memory.mode();
                let mut bytes = vec![0; if mode == Mode::X86 { 64 } else { 72 }];
                let ptr = mode as usize / 8;
                let rect_at = if mode == Mode::X86 { 8 } else { 12 };
                bytes[..ptr].copy_from_slice(&p.dc.to_le_bytes()[..ptr]);
                bytes[ptr..ptr + 4].copy_from_slice(&u32::from(!erased).to_le_bytes());
                for (n, value) in p.area.into_iter().enumerate() {
                    bytes[rect_at + n * 4..rect_at + n * 4 + 4]
                        .copy_from_slice(&value.to_le_bytes());
                }
                memory.write(p.output, &bytes)?;
                Ok(p.dc)
            }
            Kind::Query => {
                if p.output != 0 {
                    user::write_rect(memory, p.output, p.area)?;
                }
                if erased {
                    if let Some(region) = self.user.paints.get_mut(&p.window) {
                        if region.version == p.version {
                            region.erase = false;
                        }
                    }
                }
                self.user.gdi.cancel_paint(p.dc);
                Ok(u64::from(p.exists))
            }
            Kind::Default => {
                self.user.gdi.cancel_paint(p.dc);
                Ok(0)
            }
        }
    }
}
