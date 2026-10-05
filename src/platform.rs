//! Optional public-platform presentation. CPU execution never runs from a host
//! UI callback; events are queued at interpreter/API scheduling boundaries.
use crate::{api::ApiState, mem::GuestMem, thread::Sched};
use std::{
    collections::BTreeSet,
    ffi::CString,
    path::PathBuf,
    time::{Duration, Instant},
};
#[repr(C)]
#[derive(Default)]
struct Event {
    hwnd: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
}
#[cfg(target_os = "macos")]
extern "C" {
    fn wire_ui_init() -> i32;
    fn wire_ui_window(
        hwnd: u32,
        title: *const std::ffi::c_char,
        width: u32,
        height: u32,
        visible: i32,
    );
    fn wire_ui_remove(hwnd: u32);
    fn wire_ui_frame(hwnd: u32, rgba: *const u8, width: u32, height: u32) -> i32;
    fn wire_ui_poll(event: *mut Event) -> i32;
    fn wire_ui_shutdown();
}
pub struct Presenter {
    enabled: bool,
    next_poll: Instant,
    windows: BTreeSet<u32>,
    frame: u64,
    capture: Option<PathBuf>,
    captures: u32,
    last_hash: u64,
    pub displayed: u64,
    pub error: Option<String>,
}
impl Default for Presenter {
    fn default() -> Self {
        Self {
            enabled: false,
            next_poll: Instant::now(),
            windows: BTreeSet::new(),
            frame: 0,
            capture: None,
            captures: 0,
            last_hash: 0,
            displayed: 0,
            error: None,
        }
    }
}
impl Presenter {
    pub fn new(enabled: bool, capture: Option<PathBuf>) -> Result<Self, String> {
        if let Some(p) = &capture {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "macos")]
        if enabled && unsafe { wire_ui_init() } == 0 {
            return Err("AppKit requires the main thread".into());
        }
        #[cfg(not(target_os = "macos"))]
        if enabled {
            return Err("visible presenter currently requires macOS; use --headless".into());
        }
        let mut p = Self::default();
        p.enabled = enabled;
        p.capture = capture;
        Ok(p)
    }
    pub(crate) fn active(&self) -> bool {
        self.enabled
    }
    fn poll(&mut self, mem: &GuestMem, state: &mut ApiState, sched: &mut Sched) {
        if !self.enabled && self.capture.is_none() {
            return;
        }
        let now = Instant::now();
        if now < self.next_poll {
            return;
        }
        self.next_poll = now + Duration::from_millis(16);
        let live: BTreeSet<_> = state
            .user32
            .windows
            .iter()
            .filter(|(_, w)| {
                w.parent == 0 && w.style & 0x40000000 == 0 && w.client_size().iter().all(|&n| n > 1)
            })
            .map(|(&h, _)| h)
            .collect();
        #[cfg(target_os = "macos")]
        if self.enabled {
            for &h in self.windows.difference(&live) {
                unsafe { wire_ui_remove(h) }
            }
            for &h in &live {
                let w = &state.user32.windows[&h];
                let size = w.client_size();
                let title = String::from_utf16_lossy(&w.text).replace('\0', "");
                let title = CString::new(title).unwrap();
                unsafe {
                    wire_ui_window(
                        h,
                        title.as_ptr(),
                        size[0] as u32,
                        size[1] as u32,
                        (w.style & 0x10000000 != 0) as i32,
                    )
                }
            }
            for _ in 0..512 {
                let mut e = Event::default();
                if unsafe { wire_ui_poll(&mut e) } == 0 {
                    break;
                }
                crate::user32::host_message(state, sched, e.hwnd, e.message, e.wparam, e.lparam);
            }
        }
        self.windows = live;
        if state.d3d9.frames == self.frame {
            return;
        }
        self.frame = state.d3d9.frames;
        let hwnd = state.d3d9.last_presented;
        let Ok((w, h, pixels)) = state.graphics.window_snapshot(mem, hwnd) else {
            self.error = Some("presented window has no pixel surface".into());
            return;
        };
        let rgba: Vec<_> = pixels
            .iter()
            .flat_map(|&c| [c as u8, (c >> 8) as u8, (c >> 16) as u8, 255])
            .collect();
        #[cfg(target_os = "macos")]
        if self.enabled {
            if unsafe { wire_ui_frame(hwnd, rgba.as_ptr(), w, h) } != 0 {
                self.displayed += 1;
            } else {
                self.error = Some("AppKit frame upload failed".into());
            }
        }
        if let Some(p) = &self.capture {
            let hash = rgba.iter().fold(0xcbf29ce484222325u64, |n, &b| {
                (n ^ b as u64).wrapping_mul(0x100000001b3)
            });
            if self.captures < 16 && (self.captures == 0 || hash != self.last_hash) {
                let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
                for c in rgba.as_chunks::<4>().0 {
                    ppm.extend_from_slice(&c[..3]);
                }
                let path = p.join(format!("frame-{:06}.ppm", self.frame));
                if let Err(e) = std::fs::write(&path, ppm) {
                    self.error = Some(format!("capture {}: {e}", path.display()));
                } else {
                    self.captures += 1;
                    self.last_hash = hash;
                }
            }
        }
    }
}
impl Drop for Presenter {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.enabled {
            unsafe { wire_ui_shutdown() }
        }
    }
}
pub(crate) fn poll(mem: &GuestMem, state: &mut ApiState, sched: &mut Sched) {
    if !state.presenter.enabled && state.presenter.capture.is_none() {
        return;
    }
    let mut presenter = std::mem::take(&mut state.presenter);
    presenter.poll(mem, state, sched);
    state.presenter = presenter;
}
