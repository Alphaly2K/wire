//! Optional native window bridge. Guest HWNDs are mapped, never truncated to AppKit IDs.
use super::runtime::{Runtime64, WindowEvent};
use crate::host::ui;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
pub(crate) struct Presenter64 {
    windows: BTreeMap<u64, u32>,
    next_id: u32,
    next_poll: Instant,
    surfaces: BTreeMap<u64, crate::gpu::Surface>,
    capture: Option<std::path::PathBuf>,
    captured: u64,
}
impl Presenter64 {
    pub fn new() -> Result<Self, String> {
        #[cfg(not(target_os = "macos"))]
        return Err("visible Win64 presenter requires macOS".into());
        #[cfg(target_os = "macos")]
        {
            if unsafe { ui::wire_ui_init() } == 0 {
                return Err("AppKit requires the main thread".into());
            }
            Ok(Self {
                windows: BTreeMap::new(),
                next_id: 0x60000000,
                next_poll: Instant::now(),
                surfaces: BTreeMap::new(),
                capture: None,
                captured: 0,
            })
        }
    }
    pub fn capture_to(&mut self, path: &std::path::Path) {
        self.capture = Some(path.to_owned());
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
                    ui::remove_window(native);
                }
            }
            self.windows.retain(|guest, _| live.contains(guest));
            self.surfaces.retain(|guest, _| live.contains(guest));
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
                ui::update_window(ui::WindowRequest {
                    id: native,
                    title: w.title,
                    width: w.width,
                    height: w.height,
                    visible: w.visible,
                })?;
                ui::set_topmost(native, w.topmost);
            }
            for _ in 0..512 {
                let Some(event) = ui::poll_event() else {
                    break;
                };
                if let Some((&window, _)) = self.windows.iter().find(|(_, id)| **id == event.window)
                {
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
    pub fn present(
        &mut self,
        runtime: &mut Runtime64,
        chain: u64,
        interval: u32,
    ) -> Result<(), String> {
        self.poll(runtime, true)?;
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (chain, interval);
            Err("Win64 GPU presentation requires macOS".into())
        }
        #[cfg(target_os = "macos")]
        {
            let (window, texture, gpu) = runtime.graphics_frame(chain)?;
            let native = *self
                .windows
                .get(&window)
                .ok_or("no visible native window for swapchain")?;
            let layer = ui::MetalLayer::for_window(native)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = self.surfaces.entry(window) {
                entry.insert(gpu.create_surface(layer)?);
            }
            let info = runtime
                .window_info()
                .into_iter()
                .find(|w| w.handle == window && w.visible)
                .ok_or("swapchain window is hidden/destroyed")?;
            self.surfaces.get_mut(&window).unwrap().present(
                &texture,
                info.width,
                info.height,
                interval,
            )?;
            if let Some(path) = &self.capture {
                // Optional diagnostic only. Normal presentation never reads back.
                // Capture after successful submission, without converting sRGB
                // bytes a second time. File errors are not guest GPU failures.
                let result = (|| -> Result<(), String> {
                    let d = texture.desc();
                    let rgba = texture.read(0, d.width as usize * d.height as usize * 4)?;
                    let mut ppm = format!("P6\n{} {}\n255\n", d.width, d.height).into_bytes();
                    for p in rgba.as_chunks::<4>().0 {
                        ppm.extend(if matches!(d.format, 87 | 91) {
                            [p[2], p[1], p[0]]
                        } else {
                            [p[0], p[1], p[2]]
                        });
                    }
                    std::fs::write(path, ppm).map_err(|e| e.to_string())?;
                    self.captured += 1;
                    Ok(())
                })();
                if let Err(e) = result {
                    runtime.trace_note(format!(
                        "Present succeeded; diagnostic frame capture failed: {e}"
                    ));
                    self.capture = None;
                }
            }
            Ok(())
        }
    }
}
impl Drop for Presenter64 {
    fn drop(&mut self) {
        self.surfaces.clear();
        #[cfg(target_os = "macos")]
        unsafe {
            ui::wire_ui_shutdown()
        }
    }
}
