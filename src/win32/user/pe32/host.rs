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
        let [hwnd, message, wparam, lparam] = event;
        let Some(w) = self.windows.get(&hwnd) else {
            return;
        };
        let tid = w.tid;
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
                synthetic_paint: false,
                hardware_input: true,
            },
        );
    }
}
