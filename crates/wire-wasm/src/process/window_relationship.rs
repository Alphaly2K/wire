/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; LGPL 2.1 selected for wire's combined work.
 * Parent/enable flow retains the existing Wine USER derivation.
 */
//! Parent changes retain local geometry; enable notifications share the WNDPROC continuation.
use super::*;

impl Loaded {
    pub(super) fn window_relationship(
        &mut self,
        id: W,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        let a = call.arguments;
        let lookup = |h| self.user.windows.get(&h).map(Window::link);
        if id == W::Parent {
            if let Err(code) = user::hierarchy::validate_parent(a[0], a[1], lookup) {
                let result = user::error(memory, state, code)?;
                call.complete(state, result);
                return Ok(());
            }
        }
        let Some(w) = self.user.windows.get(&a[0]) else {
            let result = user::error(memory, state, 1400)?;
            call.complete(state, result);
            return Ok(());
        };
        if w.tid != u64::from(self.threads.active)
            || id == W::Parent
                && self
                    .user
                    .windows
                    .get(&a[1])
                    .is_some_and(|p| p.tid != 0 && p.tid != w.tid)
        {
            let result = user::error(memory, state, 50)?;
            call.complete(state, result);
            return Ok(());
        }
        if id == W::Enable {
            let (style, disabled, notify) = user::hierarchy::enabled(w.style, a[1] != 0);
            let messages = if notify {
                vec![[a[0], 0xa, u64::from(a[1] != 0), 0]]
            } else {
                Vec::new()
            };
            self.notifications(call, state, memory, services, messages, disabled.into())?;
            if let Some(w) = self.user.windows.get_mut(&a[0]) {
                w.style = style;
            }
            return Ok(());
        }
        let old = w.parent;
        let mut rect = w.rect;
        let mut client = w.client;
        if w.style & 0x40000000 != 0 {
            let origins = self
                .user
                .client_origin(a[0])
                .and_then(|screen| self.user.client_origin(a[1]).map(|parent| (screen, parent)));
            let (screen, parent) = match origins {
                Ok(values) => values,
                Err(code) => {
                    let value = user::error(memory, state, code)?;
                    call.complete(state, value);
                    return Ok(());
                }
            };
            let frame = [
                w.client[0].wrapping_sub(w.rect[0]),
                w.client[1].wrapping_sub(w.rect[1]),
            ];
            let position = user::hierarchy::reparent_position(screen, frame, parent);
            let delta: [i64; 2] =
                std::array::from_fn(|i| i64::from(position[i]) - i64::from(w.rect[i]));
            let shift = |r: [i32; 4]| -> Option<[i32; 4]> {
                let r = std::array::from_fn(|i| i32::try_from(i64::from(r[i]) + delta[i % 2]).ok());
                match r {
                    [Some(a), Some(b), Some(c), Some(d)] => Some([a, b, c, d]),
                    _ => None,
                }
            };
            let (Some(r), Some(c)) = (shift(rect), shift(client)) else {
                let value = user::error(memory, state, 87)?;
                call.complete(state, value);
                return Ok(());
            };
            rect = r;
            client = c;
        }
        let w = self.user.windows.get_mut(&a[0]).unwrap();
        w.parent = a[1];
        w.rect = rect;
        w.client = client;
        call.complete(state, old);
        Ok(())
    }
}
