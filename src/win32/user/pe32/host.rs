//! Owned host window snapshots and input consumption at the guest boundary.
use super::{queues::clock_ms, Message, User32};
use crate::{host::ui::WindowRequest, thread::Sched};
use std::time::Instant;

impl User32 {
    pub(crate) fn host_windows(&self, titles: bool) -> Vec<WindowRequest> {
        self.windows
            .iter()
            .filter(|(_, w)| {
                w.tid != 0
                    && w.parent == 0
                    && w.style & 0x40000000 == 0
                    && w.client_size().iter().all(|&n| n > 1)
            })
            .map(|(&id, window)| {
                let [width, height] = window.client_size();
                WindowRequest {
                    id,
                    title: if titles {
                        String::from_utf16_lossy(&window.text)
                    } else {
                        String::new()
                    },
                    width: width as u32,
                    height: height as u32,
                    visible: window.style & 0x10000000 != 0,
                }
            })
            .collect()
    }

    pub(crate) fn host_message(
        &mut self,
        last_error: &mut u32,
        sched: &mut Sched,
        start: Instant,
        event: [u32; 4],
    ) {
        let [hwnd, message, wparam, mut lparam] = event;
        let Some(w) = self.windows.get(&hwnd) else {
            return;
        };
        let tid = w.tid;
        match message {
            6 if wparam & 0xffff != 0 => self.active = hwnd,
            6 if self.active == hwnd => self.active = 0,
            7 => self.focus = hwnd,
            8 if self.focus == hwnd => self.focus = 0,
            0x1c if wparam != 0 => self.foreground = hwnd,
            0x1c if self.foreground == hwnd => self.foreground = 0,
            _ => {}
        }
        if (0x200..=0x20e).contains(&message) {
            let Ok(origin) = self.client_origin(hwnd) else {
                return;
            };
            let local = [
                lparam as u16 as i16 as i32,
                (lparam >> 16) as u16 as i16 as i32,
            ];
            let point = std::array::from_fn(|n| origin[n].wrapping_add(local[n]));
            self.cursor.set_position(point);
            if matches!(message, 0x20a | 0x20e) {
                lparam = point[0] as u16 as u32 | ((point[1] as u16 as u32) << 16);
            }
        }
        let time = clock_ms(start);
        if !self.keyboard.host([hwnd, message, wparam, lparam, time]) {
            return;
        }
        self.put_message(
            last_error,
            sched,
            tid,
            Message {
                hwnd,
                message,
                wparam,
                lparam,
                time,
                point: self.cursor.position(),
                synthetic_paint: false,
                hardware_input: true,
            },
        );
    }
}
