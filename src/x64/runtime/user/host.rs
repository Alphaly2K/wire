//! USER consumes owned host events without borrowing Runtime64 or an executor.
use super::User;
use std::time::Instant;
impl User {
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
        if self.events.len() >= 4096 {
            return Err("Win64 native window event capacity exceeded".into());
        }
        let time = start.elapsed().as_millis() as u32;
        if (0x200..=0x20e).contains(&event.message) {
            let w = &self.windows[&event.window];
            let screen = matches!(event.message, 0x20a | 0x20e);
            let input = [
                event.lparam as u16 as i16 as i32,
                (event.lparam >> 16) as u16 as i16 as i32,
            ];
            let point = std::array::from_fn(|n| {
                if screen {
                    input[n]
                } else {
                    w.client[n].wrapping_add(input[n])
                }
            });
            let point = self.cursor.host_position(point);
            let local: [i32; 2] = std::array::from_fn(|n| {
                if screen {
                    point[n]
                } else {
                    point[n].wrapping_sub(w.client[n])
                }
            });
            event.lparam = (local[0] as u16 as u64) | ((local[1] as u16 as u64) << 16);
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
        self.events.push_back(super::message::Message {
            event,
            time,
            tid: self.windows[&event.window].tid,
            point: self.cursor.position(),
            hardware: true,
        });
        Ok(())
    }
    pub(in crate::x64::runtime) fn take_window_events(&mut self) -> Vec<super::super::WindowEvent> {
        self.events.drain(..).map(|m| m.event).collect()
    }
}
