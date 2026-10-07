//! Optional native window bridge. Guest HWNDs are mapped, never truncated to AppKit IDs.
use super::runtime::{Runtime64, WindowEvent};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
    time::{Duration, Instant},
};
pub(crate) struct Presenter64 {
    windows: BTreeMap<u64, u32>,
    next_id: u32,
    next_poll: Instant,
}
impl Presenter64 {
    pub fn new() -> Result<Self, String> {
        #[cfg(not(target_os = "macos"))]
        return Err("visible Win64 presenter requires macOS".into());
        #[cfg(target_os = "macos")]
        {
            if unsafe { crate::platform::wire_ui_init() } == 0 {
                return Err("AppKit requires the main thread".into());
            }
            Ok(Self {
                windows: BTreeMap::new(),
                next_id: 0x60000000,
                next_poll: Instant::now(),
            })
        }
    }
    pub fn poll(&mut self, runtime: &mut Runtime64, force: bool) -> Result<(), String> {
        let now = Instant::now();
        if !force && now < self.next_poll {
            return Ok(());
        }
        self.next_poll = now + Duration::from_millis(16);
        let windows = runtime.window_info();
        let live: BTreeSet<_> = windows.iter().map(|w| w.handle).collect();
        #[cfg(target_os = "macos")]
        {
            for (&guest, &native) in &self.windows {
                if !live.contains(&guest) {
                    unsafe { crate::platform::wire_ui_remove(native) };
                }
            }
            self.windows.retain(|guest, _| live.contains(guest));
            for w in windows {
                if w.width == 0 || w.height == 0 {
                    continue;
                }
                let native = if let Some(&native) = self.windows.get(&w.handle) {
                    native
                } else {
                    // Keep hidden bootstrap windows as guest objects. A native
                    // window is requested only when the guest makes it visible.
                    if !w.visible {
                        continue;
                    }
                    let native = self.next_id;
                    self.next_id = self
                        .next_id
                        .checked_add(1)
                        .ok_or("native window ID exhaustion")?;
                    self.windows.insert(w.handle, native);
                    native
                };
                let title = CString::new(w.title.replace('\0', "")).map_err(|e| e.to_string())?;
                unsafe {
                    crate::platform::wire_ui_window(
                        native,
                        title.as_ptr(),
                        w.width,
                        w.height,
                        w.visible as i32,
                    )
                };
            }
            for _ in 0..512 {
                let mut event = crate::platform::Event::default();
                if unsafe { crate::platform::wire_ui_poll(&mut event) } == 0 {
                    break;
                }
                if let Some((&window, _)) = self.windows.iter().find(|(_, id)| **id == event.hwnd) {
                    runtime.queue_window_event(WindowEvent {
                        window,
                        message: event.message,
                        wparam: event.wparam as u64,
                        lparam: event.lparam as i32 as i64 as u64,
                    })?;
                }
            }
        }
        Ok(())
    }
}
impl Drop for Presenter64 {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            crate::platform::wire_ui_shutdown()
        }
    }
}
