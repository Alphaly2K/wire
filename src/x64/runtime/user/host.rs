//! USER consumes owned host events without borrowing Runtime64 or an executor.
use super::User;
use std::time::Instant;
impl User {
    pub(in crate::x64::runtime) fn configure_display(&mut self, dimensions: [u32; 2]) {
        self.display_size = dimensions;
        self.cursor = crate::win32::x64::cursor::Cursor64::new([
            0,
            0,
            dimensions[0] as i32,
            dimensions[1] as i32,
        ]);
    }
    pub(in crate::x64::runtime) fn window_info(&self) -> Vec<super::super::WindowInfo> {
        self.windows
            .iter()
            .filter(|(_, w)| w.ready && w.parent == 0)
            .map(|(&handle, w)| super::super::WindowInfo {
                handle,
                title: w.title.clone(),
                width: (w.client[2] - w.client[0]).max(0) as u32,
                height: (w.client[3] - w.client[1]).max(0) as u32,
                visible: w.style & 0x10000000 != 0,
                topmost: w.ex_style & 8 != 0,
            })
            .collect()
    }
    pub(in crate::x64::runtime) fn queue_window_event(
        &mut self,
        mut event: super::super::WindowEvent,
        start: Instant,
    ) -> Result<(), String> {
        if !self.windows.contains_key(&event.window) {
            return Ok(());
        }
        if self.events.len() > 4094 {
            return Err("Win64 native window event capacity exceeded".into());
        }
        match event.message {
            6 if event.wparam & 0xffff != 0 => self.active = event.window,
            6 if self.active == event.window => self.active = 0,
            7 => self.focus = event.window,
            8 if self.focus == event.window => self.focus = 0,
            0x1c if event.wparam != 0 => self.foreground = event.window,
            0x1c if self.foreground == event.window => self.foreground = 0,
            _ => {}
        }
        let time = start.elapsed().as_millis() as u32;
        if matches!(event.message, 0x102 | 0x106)
            && self
                .raw_input
                .registrations
                .get(&6)
                .is_some_and(|r| r.flags & 0x30 == 0x30)
        {
            return Ok(());
        }
        if (0x200..=0x20e).contains(&event.message) {
            let w = &self.windows[&event.window];
            let screen = matches!(event.message, 0x20a | 0x20e);
            let input = [
                event.lparam as u16 as i16 as i32,
                (event.lparam >> 16) as u16 as i16 as i32,
            ];
            let point = std::array::from_fn(|n| w.client[n].wrapping_add(input[n]));
            let point = self.cursor.host_position(point);
            let local: [i32; 2] = std::array::from_fn(|n| {
                if screen {
                    point[n]
                } else {
                    point[n].wrapping_sub(w.client[n])
                }
            });
            let packed = local[0] as u16 as u32 | ((local[1] as u16 as u32) << 16);
            event.lparam = packed as i32 as i64 as u64;
        }
        if !self.keyboard.host64([
            event.window,
            event.message as u64,
            event.wparam,
            event.lparam,
            time as u64,
        ]) {
            return Ok(());
        }
        if let Some((handle, no_legacy)) = self.raw_input.host(
            event.window,
            [event.message, event.wparam as u32, event.lparam as u32],
            self.cursor.position(),
            self.display_size,
            |target| self.windows.get(&target).map(|w| w.tid),
        )? {
            let packet = &self.raw_input.packets[&handle];
            let raw = super::super::WindowEvent {
                window: packet.window,
                message: 0xff,
                wparam: 0,
                lparam: handle,
            };
            crate::host::ui::trace_input("raw-queued", raw.window, 0xff, 0, handle, packet.tid);
            self.events.push_back(super::message::Message {
                event: raw,
                time,
                tid: packet.tid,
                point: self.cursor.position(),
                hardware: true,
                pointer: None,
            });
            if no_legacy
                && (!self.mouse_pointer.enabled() || !(0x200..=0x20e).contains(&event.message))
            {
                return Ok(());
            }
        }
        let pointer = self
            .mouse_pointer
            .promote(
                event.window,
                [event.message, event.wparam as u32, event.lparam as u32],
                self.cursor.position(),
                time,
            )
            .map(|(message, wp, snapshot)| {
                event.message = message;
                event.wparam = wp;
                let [x, y] = snapshot.point;
                event.lparam =
                    (u32::from(x as u16) | (u32::from(y as u16) << 16)) as i32 as i64 as u64;
                snapshot
            });
        self.events.push_back(super::message::Message {
            event,
            time,
            tid: self.windows[&event.window].tid,
            point: self.cursor.position(),
            hardware: true,
            pointer,
        });
        crate::host::ui::trace_input(
            "queued",
            event.window,
            event.message,
            event.wparam,
            event.lparam,
            self.windows[&event.window].tid,
        );
        Ok(())
    }
    pub(in crate::x64::runtime) fn take_window_events(&mut self) -> Vec<super::super::WindowEvent> {
        self.events.drain(..).map(|m| m.event).collect()
    }
}
